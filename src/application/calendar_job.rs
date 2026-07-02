use anyhow::Result;
use chrono::{DateTime, Duration, NaiveDate, Utc};

use crate::{
    app_state::AppState,
    domain::{
        activities::{ActivityKind, PlanningContext, ScheduledActivity, TimeWindow},
        calendar::CalendarEvent,
        location::Location,
        paragliding::UserSettings,
        ports::CalendarProvider,
    },
};

#[tracing::instrument(skip_all, fields(event_count = tracing::field::Empty))]
pub async fn run(state: &AppState) -> Result<()> {
    let settings = match state.site_repo.get_settings().await? {
        Some(s) => s,
        None => {
            tracing::warn!("No settings found, using defaults");
            UserSettings::default()
        }
    };

    let home = Location::new(
        settings.location_latitude,
        settings.location_longitude,
        settings.location_name.clone(),
        "".to_string(),
    );

    let cal = state.calendar.as_ref();
    cal.create_calendar(&settings.calendar_name).await?;

    let mut conflict_calendars = cal.get_calendar_names().await?;
    conflict_calendars.retain(|n| !settings.excluded_calendar_names.contains(n));

    let now = Utc::now();
    let ctx = PlanningContext {
        home,
        horizon: TimeWindow {
            start: now,
            end: now + Duration::days(14),
        },
        conflict_calendars,
    };

    let (plans, commitments) = state.planner.plan(&ctx, cal).await?;

    if let Err(e) = cal.clear_calendar(&settings.calendar_name).await {
        tracing::error!(
            calendar = %settings.calendar_name,
            error = ?e,
            "Failed to clear calendar"
        );
        return Err(e);
    }

    let mut event_counter = 0;
    for plan in plans {
        // You sleep at home every night (until camping lands), so drives are reconstructed per day:
        // depart home → first stop, hops between same-day stops, evening return → home. The chain is
        // the plan's activities *plus* the fixed commitments it was planned around, so drives route
        // through meetings (arrive by start, leave at end) instead of being drawn straight across a
        // commitment window. `prev`/`day_end` reset at each day boundary. ponytail: drives come from
        // the routing port (cached per pair) rather than being threaded through the solver's Plan.
        let mut chain = plan.items;
        chain.extend(commitments.iter().cloned());
        chain.sort_by_key(|a| a.start);
        // Fold contiguous same-site repeats of one activity into a single event; commitments have
        // distinct titles so they break runs (a meeting between two flights keeps them separate).
        let chain = coalesce(chain);

        let mut prev = ctx.home.clone();
        // The day's departure point for the evening drive home: the latest end of *any* stop that
        // day (activities and commitments), so a trailing meeting pushes the return home past it.
        let mut day_end: Option<DateTime<Utc>> = None;
        let mut cur_day: Option<NaiveDate> = None;

        for a in chain {
            let day = a.start.date_naive();
            if cur_day.is_some() && cur_day != Some(day) {
                // Day changed: drive home from where the previous day ended.
                event_counter +=
                    emit_return_home(cal, &settings.calendar_name, state, &ctx.home, &prev, day_end).await?;
                prev = ctx.home.clone();
                day_end = None;
            }
            cur_day = Some(day);

            // A located stop (activity or meeting) is a real drive waypoint; an online commitment
            // (no location) only blocks time.
            if let Some(loc) = a.location.clone() {
                if let Some(drive) = drive_between(state, &prev, &loc).await {
                    let event = drive_to_event(&prev, &loc, a.start, drive);
                    cal.create_event(&settings.calendar_name, event).await?;
                    event_counter += 1;
                }
                prev = loc;
            }
            day_end = Some(day_end.map_or(a.end, |e: DateTime<Utc>| e.max(a.end)));

            // Commitments already live on the user's own calendars — render only the drive to them.
            if a.kind != ActivityKind::Commitment {
                let event = activity_to_event(a);
                if let Err(e) = cal.create_event(&settings.calendar_name, event).await {
                    tracing::error!(error = ?e, "Failed to create event");
                    return Err(e);
                }
                event_counter += 1;
            }
        }
        // Final evening return home for the last day.
        event_counter +=
            emit_return_home(cal, &settings.calendar_name, state, &ctx.home, &prev, day_end).await?;
    }

    tracing::Span::current().record("event_count", event_counter);
    tracing::info!(
        event_count = event_counter,
        calendar = %settings.calendar_name,
        "Created events in calendar"
    );

    Ok(())
}

/// Fold consecutive stops that are the same `(kind, title, location)` and touch in time on the
/// same day into one spanning `[first.start, last.end]`. Input must be sorted by `start`. Kills
/// the objective-neutral fragmentation the GA leaves behind (several back-to-back `Do` genes for
/// one site) without merging deliberate morning/afternoon splits — a commitment between two
/// flights has a different title, so it breaks the run and keeps them separate.
fn coalesce(sorted: Vec<ScheduledActivity>) -> Vec<ScheduledActivity> {
    // ponytail: "touching" = zero gap (decode places same-site repeats exactly back-to-back); a
    // small slack absorbs second-rounding. Widen it to also swallow small Wait-induced gaps.
    let slack = Duration::minutes(1);
    let mut out: Vec<ScheduledActivity> = Vec::with_capacity(sorted.len());
    for a in sorted {
        match out.last_mut() {
            Some(prev)
                if prev.kind == a.kind
                    && prev.title == a.title
                    && loc_key(&prev.location) == loc_key(&a.location)
                    && a.start.date_naive() == prev.start.date_naive()
                    && a.start <= prev.end + slack =>
            {
                prev.end = prev.end.max(a.end);
            }
            _ => out.push(a),
        }
    }
    out
}

fn loc_key(loc: &Option<Location>) -> Option<String> {
    loc.as_ref().map(Location::to_key)
}

fn activity_to_event(a: ScheduledActivity) -> CalendarEvent {
    CalendarEvent {
        title: a.title.clone(),
        start_time: a.start,
        end_time: a.end,
        is_all_day: false,
        location: Some(a.title),
        body: Some(format!("Last updated (Utc): {}", Utc::now())),
        color_id: None,
    }
}

/// A drive leg as its own calendar event, spanning `[arrival - drive, arrival]`. Colored graphite
/// ("8") so it reads as travel, distinct from the default-colored activities.
fn drive_to_event(
    from: &Location,
    to: &Location,
    arrival: DateTime<Utc>,
    drive: Duration,
) -> CalendarEvent {
    CalendarEvent {
        title: format!("🚗 {} → {}", from.name, to.name),
        start_time: arrival - drive,
        end_time: arrival,
        is_all_day: false,
        location: None,
        body: Some(format!("{} min drive", drive.num_minutes())),
        color_id: Some("8".to_string()),
    }
}

/// Drive time between two points, or `None` (with a warning) if routing fails or the leg is zero.
async fn drive_between(state: &AppState, from: &Location, to: &Location) -> Option<Duration> {
    match state.routing.get_travel_time(from, to).await {
        Ok(d) if d > Duration::zero() => Some(d),
        Ok(_) => None,
        Err(e) => {
            tracing::warn!(from = %from.name, to = %to.name, error = %e, "drive time lookup failed; skipping drive event");
            None
        }
    }
}

/// Emit the evening drive home from `prev` (where the day's last activity sat), departing at
/// `last_end`. No-op if the day ended at home or placed no located activity.
async fn emit_return_home(
    cal: &dyn CalendarProvider,
    calendar: &str,
    state: &AppState,
    home: &Location,
    prev: &Location,
    last_end: Option<DateTime<Utc>>,
) -> Result<u32> {
    let Some(end) = last_end else { return Ok(0) };
    if prev.to_key() == home.to_key() {
        return Ok(0);
    }
    if let Some(drive) = drive_between(state, prev, home).await {
        let event = drive_to_event(prev, home, end + drive, drive);
        cal.create_event(calendar, event).await?;
        return Ok(1);
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 13, h, m, 0).unwrap()
    }
    fn loc(name: &str) -> Location {
        Location::new(50.7, 13.0, name.into(), "DE".into())
    }
    fn act(kind: ActivityKind, title: &str, location: Option<Location>, s: DateTime<Utc>, e: DateTime<Utc>) -> ScheduledActivity {
        ScheduledActivity {
            kind,
            location,
            start: s,
            end: e,
            title: title.into(),
            description: String::new(),
            fun: 0.0,
        }
    }

    #[test]
    fn coalesce_folds_contiguous_same_site_blocks() {
        let rana = || Some(loc("Rana"));
        let chain = vec![
            act(ActivityKind::Paragliding, "Rana", rana(), ts(7, 0), ts(9, 0)),
            act(ActivityKind::Paragliding, "Rana", rana(), ts(9, 0), ts(11, 0)),
            act(ActivityKind::Paragliding, "Rana", rana(), ts(11, 0), ts(15, 0)),
        ];
        let out = coalesce(chain);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].start, out[0].end), (ts(7, 0), ts(15, 0)));
    }

    #[test]
    fn coalesce_keeps_split_when_commitment_sits_between() {
        let rana = || Some(loc("Rana"));
        let chain = vec![
            act(ActivityKind::Paragliding, "Rana", rana(), ts(7, 0), ts(11, 0)),
            act(ActivityKind::Commitment, "Seminar", Some(loc("Uni")), ts(11, 0), ts(13, 0)),
            act(ActivityKind::Paragliding, "Rana", rana(), ts(13, 0), ts(15, 0)),
        ];
        let out = coalesce(chain);
        // The meeting breaks the run → two Rana blocks survive around it.
        let ranas: Vec<_> = out.iter().filter(|a| a.title == "Rana").collect();
        assert_eq!(ranas.len(), 2);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn coalesce_does_not_merge_gapped_blocks() {
        let rana = || Some(loc("Rana"));
        // A 30-min gap (> slack) with nothing between → left as two blocks.
        let chain = vec![
            act(ActivityKind::Paragliding, "Rana", rana(), ts(7, 0), ts(9, 0)),
            act(ActivityKind::Paragliding, "Rana", rana(), ts(9, 30), ts(11, 0)),
        ];
        assert_eq!(coalesce(chain).len(), 2);
    }
}
