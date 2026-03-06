import { useState, useMemo } from "react";
import "@gorules/jdm-editor/dist/style.css";
import "./styles/App.css";
import styles from "./styles/App.module.css";
import { JdmConfigProvider, DecisionGraph } from "@gorules/jdm-editor";
import { useDecisionGraph } from "./hooks/useDecisionGraph";
import { useSites, ApiSite } from "./hooks/useSites";
import { useUpdateSite, deleteSite } from "./hooks/useUpdateSite";
import { useSettings, UserSettings } from "./hooks/useSettings";
import { SitesMap } from "./components/SitesMap";
import { Header } from "./components/Header";
import { FilterPanel, Filters } from "./components/FilterPanel";
import { SiteEditor } from "./components/SiteEditor";
import { FileUploader } from "./components/FileUploader";
import { SettingsModal } from "./components/SettingsModal";

type Screen = "main" | "edit";

function App() {
  const [screen, setScreen] = useState<Screen>("main");
  const [filters, setFilters] = useState<Filters>({ siteType: "" });
  const [selectedSite, setSelectedSite] = useState<ApiSite | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [mapView, setMapView] = useState<{ center: [number, number]; zoom: number } | null>(null);
  const { graph, setGraph, loading, saving, load, save } = useDecisionGraph();
  const { sites, loading: sitesLoading, refreshing, refresh } = useSites();
  const { updateSite } = useUpdateSite();
  const { settings, updateSettings } = useSettings();

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
    if (success) {
      await refresh();
      setSelectedSite(null);
    }
  };

  const handleDeleteSite = async (siteName: string) => {
    const success = await deleteSite(siteName);
    if (success) {
      await refresh();
      setSelectedSite(null);
    }
  };

  const handleSaveSettings = async (newSettings: UserSettings) => {
    const success = await updateSettings(newSettings);
    if (success) {
      setShowSettings(false);
    }
  };

  if (loading) {
    return <div>Loading...</div>;
  }

  if (screen === "edit") {
    return (
      <JdmConfigProvider>
        <div className={styles.app}>
          <Header onLoad={load} onSave={save} saving={saving} onBack={() => setScreen("main")} />
          <div className={styles.editor}>
            <DecisionGraph value={graph} onChange={setGraph} />
          </div>
        </div>
      </JdmConfigProvider>
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
          <button className="btn" onClick={() => setScreen("edit")}>
            Edit Flyable Decision Rule
          </button>
          <button className="btn" onClick={() => setShowSettings(true)}>
            Settings
          </button>
          {sitesLoading ? null : (
            <>
              <FilterPanel
                filters={filters}
                onFilterChange={setFilters}
                sites={sites}
              />
              <FileUploader onImport={refresh} />
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
            site={selectedSite}
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
