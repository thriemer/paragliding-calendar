# Architecture

travelai follows **hexagonal architecture** (ports & adapters) with domain-driven design.

## Dependency rule

```
domain/       ← no I/O, no framework deps; defines types and port traits
application/  ← orchestrates ports; never imports concrete adapters
adapters/     ← implement ports; depend on domain, not the other way
main.rs       ← composition root; the only place adapters are wired to ports
```

The domain is the stable core. Swapping an adapter (e.g. weather provider, routing engine) means touching one file and one `Arc::new` line in `AppState`.

## Ports (`domain/ports.rs`)

| Port                    | Responsibility                                         |
| ----------------------- | ------------------------------------------------------ |
| `WeekSolver`            | Takes `SolverInput`, returns `Vec<Plan>`               |
| `ActivitySource`        | Suggests `ActivitySuggestion`s for a `PlanningContext` |
| `WeatherProvider`       | Hourly forecast + available models                     |
| `GeoProvider`           | Geocoding + elevation                                  |
| `RoutingProvider`       | Pairwise drive time between two `Location`s            |
| `CalendarProvider`      | Read/write calendar events                             |
| `SiteRepository`        | Persist/query paragliding sites                        |
| `SettingsRepository`    | User settings                                          |
| `OutdoorTourRepository` | Hiking/cycling tours                                   |
| `EventRepository`       | Dated events (festivals, etc.)                         |
| `OutdoorFeed`           | Bulk pull of upstream tour/event data (returns domain) |
| `Embedder`              | Local ONNX sentence-embedding (batch)                  |
| `EmbeddingRepository`   | Persist/read activity feature vectors                  |

## Adapters

| Adapter                     | Port(s)                           |
| --------------------------- | --------------------------------- |
| `OpenMeteoClient`           | `WeatherProvider` + `GeoProvider` |
| `Valhalla`                  | `RoutingProvider` (primary)       |
| `GraphHopper`               | `RoutingProvider` (fallback)      |
| `PostgresRepository`        | All repository traits             |
| `GoogleCalendar`            | `CalendarProvider`                |
| `MicrosoftCalendar`         | `CalendarProvider`                |
| `OutdoorActiveFeed`         | `OutdoorFeed`                     |
| `FastEmbedder`              | `Embedder` (fastembed/ONNX)       |

One concrete struct can implement multiple ports (e.g. `PostgresRepository`, `OpenMeteoClient`). Wire via separate `Arc<dyn Port>` views over one `Arc<Concrete>`.

Adapters own only **I/O and wire formats**. They live grouped by concern:
`adapters/{routing,calendar,persistence,ingest,embedding,web}/`. `ingest/` holds
the wire-format parsers (`dhv`, `kml`, `outdooractive_{tour,event}`) and the
`OutdoorActiveFeed` that downloads them. `embedding/` holds the sole ONNX site
(`FastEmbedder`); on NixOS the runtime is loaded via `ORT_DYLIB_PATH` (see
`flake.nix`/`module.nix`). Decision logic never lives here.

## Domain model (DDD)

**Aggregates**

- `ParaglidingSite` — launches, landings, metadata
- `Plan` — ordered `ScheduledActivity` items + `(total_fun, total_drive)` score

**Value objects**

- `Location` — lat/lon + name + country code
- `Score` — fun value + human-readable reasons
- `TimeWindow` — start/end `DateTime<Utc>`
- `chrono::Duration` — all drive/duration boundaries (never raw `u64`)

**Domain services** — pure decision policy, no I/O

- `scoring::paragliding` — flyable-window evaluation from site + forecast
- `scoring::tours` — per-activity weather suitability (the fun *base* now comes from the learned preference model, not editorial ratings)
- `scoring::events` — attendance policy (`MAX_ATTEND`)
- `preference_fit` — the Bradley-Terry solver: a convex multi-task objective (pairwise NLL + rating MSE + L2) hand-written for `argmin`'s LBFGS, producing per-kind `base_pref` + feature weights. `preferences::KindModel::score` applies them
- `features` — per-kind PCA + z-score for the embedding pipeline

**Application services** (`application/`) — orchestrate ports, apply scoring

- `Planner` — fans out to all `ActivitySource`s, hands `SolverInput` to `WeekSolver`
- `sources::{Paragliding,Tour,Event}ActivitySource` — implement `ActivitySource`: gather candidates from repos + weather, score them via `domain::scoring`
- `feature_job` — batch pipeline: collect activities → `Embedder` → per-kind PCA (`domain::features`) + z-score → `EmbeddingRepository`; then installs the per-kind preference-model scaffold (feature names + normalizers) and re-fits. The pure math lives in `domain::{embedding,features}`
- `preference_fit` — re-fit use case: loads comparisons/ratings/feature vectors, runs the `domain::preference_fit` solver, persists the model (`PreferenceRepository::save_model`). Called after the batch job and after every vote/rating
- `preference_scorer` — `PreferenceScorer` holds a hot-swappable snapshot (per-kind model + feature vectors) and turns `(kind, activity_id)` into a preference score. The activity sources read it synchronously in their loops (`quality` = logistic squash into `(0,1)`, folded into the weather model); reloaded after each re-fit
- `PreferenceService` — serves informative comparison pairs and records votes/ratings via `PreferenceRepository`; after each vote/rating it re-fits, reloads the scorer, and drops its cached candidate snapshot. The pure pair-selection policy is `domain::preferences::select_pair` (`GET/POST /api/preferences*`)

**Infrastructure** (not domain)

- `PersistentCache` — Postgres-backed TTL store used internally by adapters. Never appears in port signatures.

## Wiring

`AppState` holds every port as `Arc<dyn Trait>`, constructed once in `AppState::new` and cloned into Axum via `State<AppState>`. Use cases in `application/` receive `&AppState`; HTTP handlers in `adapters/http.rs` extract it and delegate.
