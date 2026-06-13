use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Duration;

use crate::{
    domain::{
        ports::ActivitySource,
        shared::{ActivityKind, ActivitySuggestion, PlanningContext, TimeWindow, Timing},
    },
    paragliding::{
        ParaglidingSiteProvider, repository::ParaglidingSiteRepository, site_evaluator,
    },
    weather::WeatherProvider,
};

pub struct ParaglidingActivitySource {
    site_repo: Arc<ParaglidingSiteRepository>,
    weather: Arc<dyn WeatherProvider>,
}

impl ParaglidingActivitySource {
    pub fn new(
        site_repo: Arc<ParaglidingSiteRepository>,
        weather: Arc<dyn WeatherProvider>,
    ) -> Self {
        Self { site_repo, weather }
    }
}

#[async_trait(?Send)]
impl ActivitySource for ParaglidingActivitySource {
    async fn suggest(&self, ctx: &PlanningContext) -> Result<Vec<ActivitySuggestion>> {
        let settings = self.site_repo.get_settings().await?.unwrap_or_default();
        let min_duration = Duration::hours(settings.minimum_flyable_hours as i64);

        let sites = self
            .site_repo
            .fetch_launches_within_radius(&ctx.home, settings.search_radius_km)
            .await;

        let mut out = Vec::new();
        for (site, _distance) in sites {
            if site.mute_alerts == Some(true) {
                tracing::info!("Skipping muted site: {}", site.name);
                continue;
            }
            let Some(launch) = site.launches.first() else {
                continue;
            };

            let forecast = match self
                .weather
                .get_forecast(launch.location.clone(), site.preferred_weather_model.as_deref())
                .await
            {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!(
                        site = %site.name,
                        lat = %launch.location.latitude,
                        lon = %launch.location.longitude,
                        error = %e,
                        "Failed to get weather forecast"
                    );
                    continue;
                }
            };

            let eval = site_evaluator::evaluate_site(&site, &forecast).await;
            for mut day in eval.daily_summaries {
                day.calculate_flyable_time_ranges();
                for range in day.ranges {
                    out.push(ActivitySuggestion {
                        kind: ActivityKind::Paragliding,
                        location: launch.location.clone(),
                        timing: Timing::Flexible {
                            window: TimeWindow {
                                start: range.start,
                                end: range.end,
                            },
                            min_duration,
                        },
                        title: site.name.clone(),
                        description: String::new(),
                        score: None,
                    });
                }
            }
        }

        Ok(out)
    }
}
