import { useState, useMemo } from "react";
import "@gorules/jdm-editor/dist/style.css";
import "./styles/App.css";
import styles from "./styles/App.module.css";
import { JdmConfigProvider, DecisionGraph } from "@gorules/jdm-editor";
import { useDecisionGraph } from "./hooks/useDecisionGraph";
import { useSites, ApiSite } from "./hooks/useSites";
import { useUpdateSite } from "./hooks/useUpdateSite";
import { SitesMap } from "./components/SitesMap";
import { Header } from "./components/Header";
import { FilterPanel, Filters } from "./components/FilterPanel";
import { SiteEditor } from "./components/SiteEditor";

type Screen = "main" | "edit";

function App() {
  const [screen, setScreen] = useState<Screen>("main");
  const [filters, setFilters] = useState<Filters>({ siteType: "" });
  const [selectedSite, setSelectedSite] = useState<ApiSite | null>(null);
  const [mapView, setMapView] = useState<{ center: [number, number]; zoom: number } | null>(null);
  const { graph, setGraph, loading, saving, load, save } = useDecisionGraph();
  const { sites, loading: sitesLoading, refreshing, refresh } = useSites();
  const { updateSite } = useUpdateSite();

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

  const handleSaveSite = async (updatedSite: ApiSite) => {
    const success = await updateSite(updatedSite);
    if (success) {
      await refresh();
      setSelectedSite(null);
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
          <button className="btn" onClick={() => setScreen("edit")}>
            Edit Flyable Decision Rule
          </button>
          {sitesLoading ? null : (
            <FilterPanel
              filters={filters}
              onFilterChange={setFilters}
              sites={sites}
            />
          )}
        </aside>
        <main className={styles.mainContent}>
          {sitesLoading ? (
            <p>Loading sites...</p>
          ) : (
            <div className={styles.mapContainer}>
              <SitesMap sites={filteredSites} onSiteClick={handleSiteClick} mapView={mapView} onMapViewChange={setMapView} />
            </div>
          )}
        </main>
      </div>
      {selectedSite && (
        <div className={styles.modalOverlay}>
          <SiteEditor
            site={selectedSite}
            onSave={handleSaveSite}
            onCancel={() => setSelectedSite(null)}
          />
        </div>
      )}
    </div>
  );
}

export { App };
export default App;
