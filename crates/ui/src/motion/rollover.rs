use crate::view::PeriodKey;

const DAY_SEC: i64 = 86400;

#[derive(Default)]
pub struct RolloverTimer {
    next_boundary_utc_ms: i64,
}

impl RolloverTimer {
    pub fn new() -> Self {
        Self { next_boundary_utc_ms: 0 }
    }

    pub fn schedule(&mut self, period: PeriodKey, week_starts_on: u8, offset_secs: i32, now_ms: i64) {
        let current_local_sec = (now_ms / 1000) + offset_secs as i64;
        
        let start_of_local_day_sec = (current_local_sec / DAY_SEC) * DAY_SEC;
        
        let next_boundary_local_sec = match period {
            PeriodKey::Day => start_of_local_day_sec + DAY_SEC,
            PeriodKey::Week => {
                let day_of_week = ((start_of_local_day_sec / DAY_SEC) + 4) % 7; 
                let mut days_to_start = (day_of_week - week_starts_on as i64) % 7;
                if days_to_start < 0 {
                    days_to_start += 7;
                }
                start_of_local_day_sec - (days_to_start * DAY_SEC) + 7 * DAY_SEC
            }
            PeriodKey::Month => {
                let (mut y, mut m, _) = sec_to_ymd(current_local_sec);
                m += 1;
                if m > 12 {
                    m = 1;
                    y += 1;
                }
                ymd_to_sec(y, m, 1)
            }
            PeriodKey::Year => {
                let (y, _, _) = sec_to_ymd(current_local_sec);
                ymd_to_sec(y + 1, 1, 1)
            }
        };
        
        self.next_boundary_utc_ms = (next_boundary_local_sec - offset_secs as i64) * 1000;
    }

    pub fn needs_refresh(&self, now_ms: i64) -> bool {
        self.next_boundary_utc_ms > 0 && now_ms >= self.next_boundary_utc_ms
    }
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

fn sec_to_ymd(secs: i64) -> (i64, u8, u8) {
    let mut days = secs / DAY_SEC;
    let mut y = 1970;
    loop {
        let dy = if is_leap(y) { 366 } else { 365 };
        if days >= dy {
            days -= dy;
            y += 1;
        } else if days < 0 {
            y -= 1;
            days += if is_leap(y) { 366 } else { 365 };
        } else {
            break;
        }
    }
    let month_lengths = [31, if is_leap(y) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0;
    while days >= month_lengths[m] {
        days -= month_lengths[m];
        m += 1;
    }
    (y, (m + 1) as u8, (days + 1) as u8)
}

fn ymd_to_sec(y: i64, m: u8, d: u8) -> i64 {
    let mut days = 0;
    for curr_y in 1970..y {
        days += if is_leap(curr_y) { 366 } else { 365 };
    }
    for curr_y in y..1970 {
        days -= if is_leap(curr_y) { 366 } else { 365 };
    }
    let month_lengths = [31, if is_leap(y) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for i in 0..(m - 1) {
        days += month_lengths[i as usize];
    }
    days += (d - 1) as i64;
    days * DAY_SEC
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rollover_boundaries() {
        let mut t = RolloverTimer::new();
        // 1970-01-01 is Thursday
        t.schedule(PeriodKey::Day, 0, 0, 0);
        assert_eq!(t.next_boundary_utc_ms, 86400 * 1000);
        
        t.schedule(PeriodKey::Month, 0, 0, 0);
        assert_eq!(t.next_boundary_utc_ms, 31 * 86400 * 1000); // Feb 1
        
        // Month lengths 28, 29, 30, 31 tested through the helper
        
        // Week boundary: 0 is Sunday, 1 is Monday.
        // Jan 1 1970 is Thursday (4). If week starts on 1 (Monday), days_to_start is (4-1)=3. Next is 1+7-3 = 5 Jan.
        t.schedule(PeriodKey::Week, 1, 0, 0);
        assert_eq!(t.next_boundary_utc_ms, 4 * 86400 * 1000);
        
        // Year end
        t.schedule(PeriodKey::Year, 0, 0, 0);
        assert_eq!(t.next_boundary_utc_ms, 365 * 86400 * 1000);

        // India offset +05:30 = 19800
        t.schedule(PeriodKey::Day, 0, 19800, 0); // 1970-01-01 05:30 IST -> Next day is 1970-01-02 00:00 IST -> 1970-01-01 18:30 UTC = 86400 - 19800 = 66600
        assert_eq!(t.next_boundary_utc_ms, (86400 - 19800) * 1000);
        
        // Nepal offset +05:45 = 20700
        t.schedule(PeriodKey::Day, 0, 20700, 0);
        assert_eq!(t.next_boundary_utc_ms, (86400 - 20700) * 1000);
    }
}
