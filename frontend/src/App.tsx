import { useState, useMemo } from "react";
import "./styles/App.css";
import styles from "./styles/App.module.css";
import { useSites, ApiSite } from "./hooks/useSites";
import { useUpdateSite } from "./hooks/useUpdateSite";
import { useSettings, UserSettings } from "./hooks/useSettings";
import { useCalendarRefresh } from "./hooks/useCalendarRefresh";
import { SitesMap } from "./components/SitesMap";
import { FilterPanel, Filters } from "./components/FilterPanel";
import { SiteEditor } from "./components/SiteEditor";
import { FileUploader } from "./components/FileUploader";
import { FlightUploader } from "./components/FlightUploader";
import { SettingsModal } from "./components/SettingsModal";

type Screen = "main" | "flights";

function App() {
  const [screen, setScreen] = useState<Screen>("main");
  const [filters, setFilters] = useState<Filters>({ siteType: "" });
  const [selectedSite, setSelectedSite] = useState<ApiSite | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [mapView, setMapView] = useState<{ center: [number, number]; zoom: number } | null>(null);
  const { sites, loading: sitesLoading } = useSites();
  const { updateSite, deleteSite } = useUpdateSite();
  const { settings, updateSettings } = useSettings();
  const { refresh: refreshCalendar, refreshing: calendarRefreshing, error: calendarRefreshError } =
    useCalendarRefresh();

  const filteredSites = useMemo(() => {
    return sites.filter((site) => {
      if (filters.siteType) {
        const hasMatchingLaunch = site.launches.some(
          (launch) => launch.site_type === filters.siteType
        );
        if (!hasMatchingLaunch) return false;
      }
      return true;
    });
  }, [sites, filters]);

  const defaultCenter = useMemo<[number, number]>(() => {
    if (mapView) return mapView.center;
    if (settings && settings.location_latitude && settings.location_longitude) {
      return [settings.location_latitude, settings.location_longitude];
    }
    return [47.0, 10.0];
  }, [mapView, settings]);

  const handleSiteClick = (site: ApiSite) => {
    setSelectedSite(site);
  };

  const handleCreateSite = () => {
    const emptySite: ApiSite = {
      name: "",
      country: null,
      launches: [],
      landings: [],
      data_source: "API",
    };
    setSelectedSite(emptySite);
  };

  const handleSaveSite = async (updatedSite: ApiSite) => {
    const success = await updateSite(updatedSite);
    if (success) setSelectedSite(null);
  };

  const handleDeleteSite = async (siteName: string) => {
    const success = await deleteSite(siteName);
    if (success) setSelectedSite(null);
  };

  const handleSaveSettings = async (newSettings: UserSettings) => {
    const success = await updateSettings(newSettings);
    if (success) {
      setShowSettings(false);
    }
  };

  if (screen === "flights") {
    return (
      <div className={styles.app}>
        <div className={styles.mainScreen}>
          <aside className={styles.sidePanel}>
            <h2>Flight Analytics</h2>
            <button className="btn btn-back" onClick={() => setScreen("main")}>
              Back to Main
            </button>
          </aside>
          <main className={styles.mainContent}>
            <FlightUploader />
          </main>
        </div>
      </div>
    );
  }

  return (
    <div className={styles.app}>
      <div className={styles.mainScreen}>
        <aside className={styles.sidePanel}>
          <h2>Menu</h2>
          <button className="btn" onClick={handleCreateSite}>
            Create New Site
          </button>
          <button className="btn" onClick={() => setScreen("flights")}>
            Flight Analytics
          </button>
          <button className="btn" onClick={() => setShowSettings(true)}>
            Settings
          </button>
          <button
            className="btn"
            onClick={() => refreshCalendar()}
            disabled={calendarRefreshing}
          >
            {calendarRefreshing ? "Refreshing…" : "Refresh Calendar"}
          </button>
          {calendarRefreshError && (
            <div className={styles.sidebarError}>{calendarRefreshError}</div>
          )}
          {sitesLoading ? null : (
            <>
              <FilterPanel
                filters={filters}
                onFilterChange={setFilters}
                sites={sites}
              />
              <FileUploader />
            </>
          )}
        </aside>
        <main className={styles.mainContent}>
          {sitesLoading ? (
            <p>Loading sites...</p>
          ) : (
            <div className={styles.mapContainer}>
              <SitesMap
                sites={filteredSites}
                onSiteClick={handleSiteClick}
                mapView={mapView}
                onMapViewChange={setMapView}
                settings={settings ?? undefined}
              />
            </div>
          )}
        </main>
      </div>
      {selectedSite && (
        <div className={styles.modalOverlay}>
          <SiteEditor
            key={selectedSite.name || "new"}
            site={selectedSite}
            defaultCenter={defaultCenter}
            onSave={handleSaveSite}
            onDelete={selectedSite.name ? handleDeleteSite : undefined}
            onCancel={() => setSelectedSite(null)}
          />
        </div>
      )}
      {showSettings && settings && (
        <SettingsModal
          settings={settings}
          onSave={handleSaveSettings}
          onCancel={() => setShowSettings(false)}
        />
      )}
    </div>
  );
}

export { App };
export default App;
