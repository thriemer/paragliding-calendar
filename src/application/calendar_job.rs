use anyhow::Result;
use chrono::{Duration, Utc};

use crate::{
    app_state::AppState,
    domain::{
        activities::{PlanningContext, ScheduledActivity, TimeWindow},
        calendar::CalendarEvent,
        location::Location,
        paragliding::UserSettings,
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

    let plans = state.planner.plan(&ctx, cal).await?;

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
        for a in plan.items {
            let event = activity_to_event(a);
            if let Err(e) = cal.create_event(&settings.calendar_name, event).await {
                tracing::error!(error = ?e, "Failed to create event");
                return Err(e);
            }
            event_counter += 1;
        }
    }

    tracing::Span::current().record("event_count", event_counter);
    tracing::info!(
        event_count = event_counter,
        calendar = %settings.calendar_name,
        "Created events in calendar"
    );

    Ok(())
}

fn activity_to_event(a: ScheduledActivity) -> CalendarEvent {
    CalendarEvent {
        title: a.title.clone(),
        start_time: a.start,
        end_time: a.end,
        is_all_day: false,
        location: Some(a.title),
        body: Some(format!("Last updated (Utc): {}", Utc::now())),
    }
}
