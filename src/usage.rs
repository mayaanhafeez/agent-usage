use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Days, Local, NaiveDate, Utc};
use reqwest::blocking::Client;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use walkdir::WalkDir;

use crate::model::{Limit, Provider, Snapshot, UsageRow};

const HISTORY_DAYS: u64 = 7;

pub fn collect(provider: Provider) -> Snapshot {
    let mut snapshot = Snapshot::empty(provider);
    let result = match provider {
        Provider::Claude => collect_claude(&mut snapshot),
        Provider::Codex => collect_codex(&mut snapshot),
        Provider::OpenCode => collect_opencode(&mut snapshot),
        Provider::Gemini => collect_gemini(&mut snapshot),
    };
    if let Err(error) = result {
        snapshot.note = Some(error.to_string());
    }
    snapshot
}

pub fn collect_all(providers: &[Provider]) -> Vec<Snapshot> {
    let handles: Vec<_> = providers
        .iter()
        .copied()
        .map(|provider| std::thread::spawn(move || collect(provider)))
        .collect();
    handles
        .into_iter()
        .map(|handle| handle.join().expect("usage collector panicked"))
        .collect()
}

fn home() -> Result<PathBuf> {
    dirs::home_dir().context("home directory is unavailable")
}

fn recent_days() -> (NaiveDate, NaiveDate) {
    let today = Local::now().date_naive();
    (
        today
            .checked_sub_days(Days::new(HISTORY_DAYS - 1))
            .unwrap_or(today),
        today,
    )
}

fn finish_rows(
    snapshot: &mut Snapshot,
    by_day: BTreeMap<NaiveDate, u64>,
    by_model: HashMap<String, u64>,
) {
    let (oldest, today) = recent_days();
    snapshot.days = (0..HISTORY_DAYS)
        .map(|offset| oldest.checked_add_days(Days::new(offset)).unwrap_or(oldest))
        .map(|day| UsageRow {
            label: if day == today {
                "Today".into()
            } else {
                day.format("%a").to_string()
            },
            tokens: by_day.get(&day).copied().unwrap_or(0),
        })
        .collect();
    let mut models: Vec<_> = by_model
        .into_iter()
        .map(|(label, tokens)| UsageRow { label, tokens })
        .collect();
    models.sort_by_key(|row| std::cmp::Reverse(row.tokens));
    models.truncate(6);
    snapshot.models = models;
}

fn recent_jsonl(root: &Path) -> Vec<PathBuf> {
    let cutoff = SystemTime::now() - Duration::from_secs((HISTORY_DAYS + 1) * 86_400);
    if !root.is_dir() {
        return Vec::new();
    }
    WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        .filter(|entry| {
            entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .is_some_and(|time| time >= cutoff)
        })
        .map(|entry| entry.into_path())
        .collect()
}

fn parse_time(value: &Value) -> Option<DateTime<Local>> {
    value
        .as_str()?
        .parse::<DateTime<Utc>>()
        .ok()
        .map(|time| time.with_timezone(&Local))
}

fn collect_claude(snapshot: &mut Snapshot) -> Result<()> {
    let claude_dir = home()?.join(".claude");
    let credentials = claude_credentials(&claude_dir);
    if let Some(creds) = &credentials {
        snapshot.plan = creds
            .pointer("/claudeAiOauth/subscriptionType")
            .or_else(|| creds.pointer("/subscriptionType"))
            .and_then(Value::as_str)
            .map(pretty_plan);
        if let Some(token) = creds
            .pointer("/claudeAiOauth/accessToken")
            .and_then(Value::as_str)
            && let Ok(raw) = fetch_json(
                "https://api.anthropic.com/api/oauth/usage",
                token,
                &[("anthropic-beta", "oauth-2025-04-20")],
            )
        {
            for (label, needles) in [
                ("Session", ["five_hour", "session"]),
                ("Weekly", ["seven_day", "week"]),
            ] {
                if let Some(window) = find_window(&raw, &needles)
                    && let Some(percent) = window_percent(window)
                {
                    snapshot.limits.push(Limit {
                        label: label.into(),
                        percent,
                        resets: window_reset(window),
                    });
                }
            }
        }
    }

    let mut by_day = BTreeMap::new();
    let mut by_model = HashMap::new();
    let mut seen = HashSet::new();
    let (oldest, today) = recent_days();
    for path in recent_jsonl(&claude_dir.join("projects")) {
        let Ok(file) = File::open(&path) else {
            continue;
        };
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if !line.contains("\"requestId\"") {
                continue;
            }
            let Ok(record) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if record.get("type").and_then(Value::as_str) != Some("assistant") {
                continue;
            }
            let id = record
                .get("requestId")
                .or_else(|| record.get("uuid"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if !seen.insert((path.clone(), id.to_owned())) {
                continue;
            }
            let Some(day) = record
                .get("timestamp")
                .and_then(parse_time)
                .map(|t| t.date_naive())
            else {
                continue;
            };
            if day < oldest || day > today {
                continue;
            }
            let Some(message) = record.get("message") else {
                continue;
            };
            let Some(usage) = message.get("usage") else {
                continue;
            };
            let tokens = weighted_claude_tokens(usage);
            *by_day.entry(day).or_default() += tokens;
            if let Some(model) = message
                .get("model")
                .and_then(Value::as_str)
                .and_then(pretty_claude_model)
            {
                *by_model.entry(model).or_default() += tokens;
            }
        }
    }
    finish_rows(snapshot, by_day, by_model);
    if credentials.is_none() {
        snapshot.note = Some("Claude credentials not found; showing local history".into());
    }
    Ok(())
}

fn claude_credentials(claude_dir: &Path) -> Option<Value> {
    #[cfg(target_os = "macos")]
    if let Ok(output) = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .output()
        && output.status.success()
        && let Ok(value) = serde_json::from_slice(&output.stdout)
    {
        return Some(value);
    }
    fs::read(claude_dir.join(".credentials.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

fn weighted_claude_tokens(usage: &Value) -> u64 {
    let n = |key| usage.get(key).and_then(Value::as_u64).unwrap_or(0) as f64;
    (n("input_tokens")
        + n("output_tokens")
        + n("cache_creation_input_tokens") * 1.25
        + n("cache_read_input_tokens") * 0.1)
        .round() as u64
}

fn pretty_claude_model(model: &str) -> Option<String> {
    let lower = model.to_ascii_lowercase();
    if lower.is_empty() || lower == "<synthetic>" {
        return None;
    }
    for (needle, name) in [
        ("opus-5", "Opus 5"),
        ("sonnet-5", "Sonnet 5"),
        ("fable-5", "Fable 5"),
        ("opus", "Opus"),
        ("sonnet", "Sonnet"),
        ("haiku", "Haiku"),
    ] {
        if lower.contains(needle) {
            return Some(name.into());
        }
    }
    Some(model.into())
}

fn collect_codex(snapshot: &mut Snapshot) -> Result<()> {
    let codex_dir = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or(home()?.join(".codex"));
    apply_chatgpt_account(snapshot, &codex_dir);

    let mut by_day = BTreeMap::new();
    let mut by_model = HashMap::new();
    let (oldest, today) = recent_days();
    for path in recent_jsonl(&codex_dir.join("sessions")) {
        let Ok(file) = File::open(path) else { continue };
        let mut model = "unknown".to_owned();
        let mut previous_total = 0_u64;
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if !line.contains("token_count")
                && !line.contains("thread_settings_applied")
                && !line.contains("session_meta")
            {
                continue;
            }
            let Ok(record) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let payload = &record["payload"];
            if let Some(value) = payload
                .pointer("/thread_settings/model")
                .or_else(|| payload.get("model"))
                .and_then(Value::as_str)
            {
                model = value.to_owned();
            }
            if payload.get("type").and_then(Value::as_str) != Some("token_count") {
                continue;
            }
            let Some(total) = payload
                .pointer("/info/total_token_usage/total_tokens")
                .and_then(Value::as_u64)
            else {
                continue;
            };
            let delta = total.saturating_sub(previous_total);
            previous_total = total;
            if delta == 0 {
                continue;
            }
            let Some(day) = record
                .get("timestamp")
                .and_then(parse_time)
                .map(|t| t.date_naive())
            else {
                continue;
            };
            if day < oldest || day > today {
                continue;
            }
            *by_day.entry(day).or_default() += delta;
            *by_model.entry(model.clone()).or_default() += delta;
        }
    }
    finish_rows(snapshot, by_day, by_model);
    Ok(())
}

fn collect_opencode(snapshot: &mut Snapshot) -> Result<()> {
    let home = home()?;
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .into_iter()
        .chain([home.join(".local/share")])
        .chain(dirs::data_local_dir())
        .map(|root| root.join("opencode/opencode.db"))
        .find(|path| path.exists())
        .context("OpenCode database not found")?;
    let db = Connection::open_with_flags(&data, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .context("cannot open OpenCode database")?;
    let mut statement = db.prepare("SELECT time_updated, model, tokens_input + tokens_output + tokens_reasoning + tokens_cache_read + tokens_cache_write FROM session WHERE time_archived IS NULL")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, u64>(2)?,
        ))
    })?;
    let mut by_day = BTreeMap::new();
    let mut by_model = HashMap::new();
    let (oldest, today) = recent_days();
    for row in rows.flatten() {
        let Some(time) = DateTime::from_timestamp_millis(row.0).map(|t| t.with_timezone(&Local))
        else {
            continue;
        };
        let day = time.date_naive();
        if day < oldest || day > today || row.2 == 0 {
            continue;
        }
        *by_day.entry(day).or_default() += row.2;
        if let Some(model) = row.1 {
            *by_model.entry(display_opencode_model(&model)).or_default() += row.2;
        }
    }
    let uses_chatgpt = by_model.keys().any(|model| {
        let model = model.to_ascii_lowercase();
        model.starts_with("gpt-") || model.contains("openai/") || model.contains("codex")
    });
    finish_rows(snapshot, by_day, by_model);
    if uses_chatgpt {
        let codex_dir = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or(home.join(".codex"));
        apply_chatgpt_account(snapshot, &codex_dir);
    }
    snapshot.note = Some(if snapshot.limits.is_empty() {
        "Provider quotas vary by model; local token totals shown".into()
    } else {
        "ChatGPT quota via local Codex login".into()
    });
    Ok(())
}

fn apply_chatgpt_account(snapshot: &mut Snapshot, codex_dir: &Path) {
    let Ok(auth) = fs::read(codex_dir.join("auth.json"))
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).map_err(std::io::Error::other))
    else {
        return;
    };
    if let Some(id_token) = auth.pointer("/tokens/id_token").and_then(Value::as_str) {
        snapshot.plan = jwt_claims(id_token).and_then(|claims| {
            claims
                .get("https://api.openai.com/auth")
                .and_then(|auth| auth.get("chatgpt_plan_type"))
                .and_then(Value::as_str)
                .map(pretty_plan)
        });
    }
    let Some(token) = auth.pointer("/tokens/access_token").and_then(Value::as_str) else {
        return;
    };
    let mut headers = Vec::new();
    if let Some(account) = auth.pointer("/tokens/account_id").and_then(Value::as_str) {
        headers.push(("ChatGPT-Account-Id", account));
    }
    if let Ok(raw) = fetch_json(
        "https://chatgpt.com/backend-api/wham/usage",
        token,
        &headers,
    ) && let Some(window) = raw.pointer("/rate_limit/primary_window")
        && let Some(percent) = window.get("used_percent").and_then(Value::as_f64)
    {
        snapshot.limits.push(Limit {
            label: "Weekly".into(),
            percent: percent.round().clamp(0.0, 100.0) as u16,
            resets: reset_after(window),
        });
    }
}

fn display_opencode_model(model: &str) -> String {
    serde_json::from_str::<Value>(model)
        .ok()
        .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| model.to_owned())
}

fn collect_gemini(snapshot: &mut Snapshot) -> Result<()> {
    let root = home()?.join(".gemini/tmp");
    if !root.is_dir() {
        anyhow::bail!("Gemini CLI history not found");
    }
    let mut by_day = BTreeMap::new();
    let mut by_model = HashMap::new();
    let (oldest, today) = recent_days();
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
    {
        let Ok(value) = fs::read(entry.path())
            .and_then(|b| serde_json::from_slice::<Value>(&b).map_err(std::io::Error::other))
        else {
            continue;
        };
        let Some(messages) = value.get("messages").and_then(Value::as_array) else {
            continue;
        };
        for message in messages {
            let Some(tokens) = message.get("tokens") else {
                continue;
            };
            let total = ["input", "output", "cached", "thoughts", "tool"]
                .iter()
                .filter_map(|k| tokens.get(k).and_then(Value::as_u64))
                .sum::<u64>();
            let Some(day) = message
                .get("timestamp")
                .and_then(parse_time)
                .map(|t| t.date_naive())
            else {
                continue;
            };
            if day < oldest || day > today || total == 0 {
                continue;
            }
            *by_day.entry(day).or_default() += total;
            let model = message
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("Gemini");
            *by_model.entry(model.to_owned()).or_default() += total;
        }
    }
    finish_rows(snapshot, by_day, by_model);
    snapshot.note = Some("Gemini quota details are not exposed by the CLI".into());
    Ok(())
}

fn fetch_json(url: &str, token: &str, headers: &[(&str, &str)]) -> Result<Value> {
    let client = Client::builder().timeout(Duration::from_secs(4)).build()?;
    let mut request = client
        .get(url)
        .bearer_auth(token)
        .header("User-Agent", "agent-usage/0.1");
    for (key, value) in headers {
        request = request.header(*key, *value);
    }
    Ok(request.send()?.error_for_status()?.json()?)
}

fn jwt_claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()
}

fn pretty_plan(plan: &str) -> String {
    plan.split('_')
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        })
        .collect::<Vec<String>>()
        .join(" ")
}

fn find_window<'a>(raw: &'a Value, needles: &[&str]) -> Option<&'a Value> {
    raw.as_object()?
        .iter()
        .find(|(key, value)| {
            value.is_object()
                && needles
                    .iter()
                    .any(|needle| key.to_ascii_lowercase().contains(needle))
        })
        .map(|(_, value)| value)
}

fn window_percent(window: &Value) -> Option<u16> {
    for key in ["utilization", "percent_used", "percentage", "used_percent"] {
        if let Some(value) = window.get(key).and_then(Value::as_f64) {
            let percent = if (0.0..=1.0).contains(&value) {
                value * 100.0
            } else {
                value
            };
            return Some(percent.round().clamp(0.0, 100.0) as u16);
        }
    }
    let used = window.get("used").and_then(Value::as_f64)?;
    let limit = window.get("limit").and_then(Value::as_f64)?;
    (limit > 0.0).then(|| (used / limit * 100.0).round().clamp(0.0, 100.0) as u16)
}

fn window_reset(window: &Value) -> Option<String> {
    ["resets_at", "reset_at", "resets", "expires_at"]
        .into_iter()
        .find_map(|key| {
            let value = window.get(key)?;
            let when = if let Some(epoch) = value.as_i64() {
                DateTime::from_timestamp(epoch, 0)?
            } else {
                value.as_str()?.parse::<DateTime<Utc>>().ok()?
            };
            Some(human_duration((when - Utc::now()).num_seconds()))
        })
}

fn reset_after(window: &Value) -> Option<String> {
    window
        .get("reset_after_seconds")
        .and_then(Value::as_i64)
        .map(human_duration)
        .or_else(|| {
            window
                .get("reset_at")
                .and_then(Value::as_i64)
                .map(|epoch| human_duration(epoch - Utc::now().timestamp()))
        })
}

fn human_duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let days = seconds / 86_400;
    let hours = seconds % 86_400 / 3_600;
    let minutes = seconds % 3_600 / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{}m", minutes.max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages_accept_fraction_or_whole() {
        assert_eq!(
            window_percent(&serde_json::json!({"utilization": 0.42})),
            Some(42)
        );
        assert_eq!(
            window_percent(&serde_json::json!({"used_percent": 77})),
            Some(77)
        );
        assert_eq!(
            window_percent(&serde_json::json!({"used": 3, "limit": 4})),
            Some(75)
        );
    }

    #[test]
    fn durations_are_compact() {
        assert_eq!(human_duration(6 * 86_400 + 13 * 3_600), "6d 13h");
        assert_eq!(human_duration(4 * 3_600 + 37 * 60), "4h 37m");
    }
}
