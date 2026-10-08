use super::types::*;
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use rust_decimal::Decimal;
use serde_json::Value;
use std::str::FromStr;

pub fn timezone(name: &str) -> Result<Tz, String> {
    name.parse()
        .map_err(|_| "统计时区无效，请使用 IANA 时区名称。".into())
}
pub fn midnight(date: NaiveDate, tz: Tz) -> Result<DateTime<Utc>, String> {
    // A few zones skip midnight during historical DST transitions: choose the first valid minute.
    let base = date.and_hms_opt(0, 0, 0).ok_or("日期无效")?;
    for minute in 0..180 {
        if let Some(t) = tz
            .from_local_datetime(&(base + Duration::minutes(minute)))
            .earliest()
        {
            return Ok(t.with_timezone(&Utc));
        }
    }
    Err("无法确定统计日期边界。".into())
}
pub fn bounds(
    range: UsageRange,
    minutes: u32,
    tz: Tz,
    now: DateTime<Utc>,
) -> Result<(DateTime<Utc>, DateTime<Utc>), String> {
    let local = now.with_timezone(&tz);
    let start = match range {
        UsageRange::Today => midnight(local.date_naive(), tz)?,
        UsageRange::Month => midnight(local.date_naive().with_day(1).ok_or("日期无效")?, tz)?,
        UsageRange::Week => now - Duration::hours(168),
        UsageRange::Recent => now - Duration::minutes(i64::from(minutes)),
    };
    Ok((start, now))
}
pub fn trend_bucket(
    time: DateTime<Utc>,
    range: UsageRange,
    tz: Tz,
) -> Result<DateTime<Utc>, String> {
    let local = time.with_timezone(&tz);
    // Elapsed subtraction preserves both occurrences of a repeated local hour.
    // Calendar-day buckets still use the actual local midnight (23/25-hour days).
    match range {
        UsageRange::Recent => Ok(time
            - Duration::seconds(i64::from(local.second()))
            - Duration::nanoseconds(i64::from(local.nanosecond()))),
        UsageRange::Today => Ok(time
            - Duration::seconds(i64::from(local.minute() * 60 + local.second()))
            - Duration::nanoseconds(i64::from(local.nanosecond()))),
        _ => midnight(local.date_naive(), tz),
    }
}
pub fn decimal(v: &Value) -> Result<Decimal, String> {
    let text = if let Some(s) = v.as_str() {
        s.to_owned()
    } else if v.is_number() {
        v.to_string()
    } else {
        return Err("站点缺少有效金额字段。".into());
    };
    Decimal::from_str(&text)
        .or_else(|_| Decimal::from_scientific(&text))
        .map_err(|_| "站点金额格式不支持。".into())
}
fn number(v: &Value, k: &str) -> Result<u64, String> {
    v.get(k)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("站点缺少统计字段 {k}，当前版本无法确认统计口径。"))
}
impl Totals {
    pub fn zero() -> Self {
        Self {
            cost: "0".into(),
            ..Self::default()
        }
    }
    pub fn recalculate(&mut self) {
        self.total_tokens = self.input_tokens
            + self.output_tokens
            + self.cache_read_tokens
            + self.cache_creation_tokens;
        let input = self.input_tokens + self.cache_read_tokens + self.cache_creation_tokens;
        self.cache_rate = (input > 0).then(|| self.cache_read_tokens as f64 / input as f64);
    }
    pub fn add(&mut self, other: &Self) {
        self.cost = (Decimal::from_str(&self.cost).unwrap_or_default()
            + Decimal::from_str(&other.cost).unwrap_or_default())
        .normalize()
        .to_string();
        self.requests += other.requests;
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_creation_tokens += other.cache_creation_tokens;
        self.recalculate();
    }
    pub fn from_summary(v: &Value) -> Result<Self, String> {
        let mut t = Self {
            cost: decimal(&v["total_actual_cost"])?.normalize().to_string(),
            requests: number(v, "total_requests")?,
            input_tokens: number(v, "total_input_tokens")?,
            output_tokens: number(v, "total_output_tokens")?,
            cache_read_tokens: number(v, "total_cache_read_tokens")?,
            cache_creation_tokens: number(v, "total_cache_creation_tokens")?,
            ..Self::default()
        };
        t.recalculate();
        Ok(t)
    }
    pub fn from_chart(v: &Value) -> Result<Self, String> {
        let mut t = Self {
            cost: decimal(&v["actual_cost"])?.normalize().to_string(),
            requests: number(v, "requests")?,
            input_tokens: number(v, "input_tokens")?,
            output_tokens: number(v, "output_tokens")?,
            cache_read_tokens: number(v, "cache_read_tokens")?,
            cache_creation_tokens: number(v, "cache_creation_tokens")?,
            ..Self::default()
        };
        t.recalculate();
        Ok(t)
    }
}
pub fn record(v: &Value) -> Result<UsageRecord, String> {
    let mut t = Totals {
        cost: decimal(&v["actual_cost"])?.normalize().to_string(),
        requests: 1,
        input_tokens: number(v, "input_tokens")?,
        output_tokens: number(v, "output_tokens")?,
        cache_read_tokens: number(v, "cache_read_tokens")?,
        cache_creation_tokens: number(v, "cache_creation_tokens")?,
        ..Totals::default()
    };
    t.recalculate();
    let date = v["created_at"].as_str().ok_or("记录缺少时间")?;
    DateTime::parse_from_rfc3339(date).map_err(|_| "站点记录时间格式不支持。")?;
    Ok(UsageRecord {
        id: v["id"].as_i64().ok_or("记录缺少 ID")?,
        created_at: date.into(),
        model: v["model"].as_str().unwrap_or("未知模型").into(),
        api_key_id: v["api_key_id"].as_i64().ok_or("记录缺少 Key ID")?,
        api_key_name: v["api_key"]["name"]
            .as_str()
            .unwrap_or("已删除或未命名 Key")
            .into(),
        totals: t,
        duration_ms: v["duration_ms"].as_f64(),
        stream: v["stream"].as_bool().unwrap_or(false),
    })
}
pub fn within(r: &UsageRecord, start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
    DateTime::parse_from_rfc3339(&r.created_at).is_ok_and(|d| d >= start && d < end)
}
pub fn aggregate(records: &[UsageRecord], start: DateTime<Utc>, end: DateTime<Utc>) -> Totals {
    let mut t = Totals::zero();
    for r in records.iter().filter(|r| within(r, start, end)) {
        t.add(&r.totals);
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calendar_and_dst() {
        let tz: Tz = "America/New_York".parse().unwrap();
        let now = DateTime::parse_from_rfc3339("2024-03-11T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let (s, e) = bounds(UsageRange::Week, 5, tz, now).unwrap();
        assert_eq!((e - s).num_hours(), 168);
        let (s, _) = bounds(UsageRange::Month, 5, tz, now).unwrap();
        assert_eq!(s.to_rfc3339(), "2024-03-01T05:00:00+00:00");
        let leap = DateTime::parse_from_rfc3339("2024-02-29T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            bounds(UsageRange::Today, 5, "Asia/Shanghai".parse().unwrap(), leap)
                .unwrap()
                .0
                .to_rfc3339(),
            "2024-02-29T16:00:00+00:00"
        );
    }
    #[test]
    fn weighted_cache_and_exact_cost() {
        let mut t = Totals::zero();
        t.add(&Totals {
            cost: "0.1".into(),
            input_tokens: 90,
            cache_read_tokens: 10,
            output_tokens: 100,
            requests: 1,
            ..Totals::default()
        });
        t.add(&Totals {
            cost: "0.2".into(),
            cache_read_tokens: 900,
            cache_creation_tokens: 100,
            requests: 1,
            ..Totals::default()
        });
        assert_eq!(t.cost, "0.3");
        assert_eq!(t.total_tokens, 1200);
        assert_eq!(t.cache_rate, Some(910.0 / 1100.0));
        assert_eq!(Totals::zero().cache_rate, None);
    }
    #[test]
    fn trend_buckets_keep_repeated_dst_hours_and_calendar_midnights() {
        let tz = "America/New_York".parse().unwrap();
        let first = DateTime::parse_from_rfc3339("2026-11-01T01:35:42-04:00")
            .unwrap()
            .with_timezone(&Utc);
        let second = DateTime::parse_from_rfc3339("2026-11-01T01:35:42-05:00")
            .unwrap()
            .with_timezone(&Utc);
        let before = trend_bucket(first, UsageRange::Today, tz).unwrap();
        let after = trend_bucket(second, UsageRange::Today, tz).unwrap();
        assert_eq!((after - before).num_hours(), 1);
        assert_eq!(before.with_timezone(&tz).hour(), 1);
        assert_eq!(after.with_timezone(&tz).hour(), 1);
        let day = trend_bucket(first, UsageRange::Week, tz).unwrap();
        let next_day = trend_bucket(second + Duration::days(1), UsageRange::Week, tz).unwrap();
        assert_eq!((next_day - day).num_hours(), 25);
    }
    #[test]
    fn unsupported_fields_fail_closed() {
        assert!(Totals::from_summary(&serde_json::json!({"total_requests":1})).is_err());
    }
    #[test]
    fn half_open_window_expires_without_new_rows() {
        let now = Utc::now();
        let r = UsageRecord {
            id: 1,
            created_at: (now - Duration::minutes(5)).to_rfc3339(),
            model: "a".into(),
            api_key_id: 1,
            api_key_name: "a".into(),
            totals: Totals {
                requests: 1,
                ..Totals::zero()
            },
            duration_ms: None,
            stream: false,
        };
        assert_eq!(
            aggregate(std::slice::from_ref(&r), now - Duration::minutes(5), now).requests,
            1
        );
        assert_eq!(
            aggregate(&[r], now - Duration::minutes(5) + Duration::seconds(1), now).requests,
            0
        );
    }
}
