import styles from "./SitesMap.module.css";

// Swatch colors match the leaflet-color-markers pins used in SitesMap.
const PIN = {
  winch: "#2A81CB", // blue
  hang: "#2AAD27", // green
  both: "#9C2BCB", // violet
  landing: "#CB2B3E", // red
  user: "#CB8427", // orange
};

interface LegendProps {
  isZoomedIn: boolean;
  hasLocationSettings?: boolean;
}

export function Legend({ isZoomedIn, hasLocationSettings }: LegendProps) {
  return (
    <div className={styles.legend}>
      {hasLocationSettings && (
        <div className={styles.legendItem}>
          <span className={styles.legendColor} style={{ backgroundColor: PIN.user }}></span>
          Your Location
        </div>
      )}
      {hasLocationSettings && (
        <div className={styles.legendItem}>
          <span className={styles.legendDashed}></span>
          Search Radius
        </div>
      )}
      {isZoomedIn ? (
        <>
          <div className={styles.legendItem}>
            <span className={styles.legendColor} style={{ backgroundColor: PIN.winch }}></span>
            Winch Launch
          </div>
          <div className={styles.legendItem}>
            <span className={styles.legendColor} style={{ backgroundColor: PIN.hang }}></span>
            Hang Launch
          </div>
          <div className={styles.legendItem}>
            <span className={styles.legendColor} style={{ backgroundColor: PIN.landing }}></span>
            Landing
          </div>
        </>
      ) : (
        <>
          <div className={styles.legendItem}>
            <span className={styles.legendColor} style={{ backgroundColor: PIN.winch }}></span>
            Winch
          </div>
          <div className={styles.legendItem}>
            <span className={styles.legendColor} style={{ backgroundColor: PIN.hang }}></span>
            Hang
          </div>
          <div className={styles.legendItem}>
            <span className={styles.legendColor} style={{ backgroundColor: PIN.both }}></span>
            Winch + Hang
          </div>
        </>
      )}
    </div>
  );
}
