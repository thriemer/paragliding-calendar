import styles from "./SitesMap.module.css";

interface LegendProps {
  isZoomedIn: boolean;
  hasLocationSettings?: boolean;
}

export function Legend({ isZoomedIn, hasLocationSettings }: LegendProps) {
  return (
    <div className={styles.legend}>
      {hasLocationSettings && (
        <div className={styles.legendItem}>
          <span className={styles.legendColor} style={{ backgroundColor: "#ff9800" }}></span>
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
            <span className={styles.legendColor} style={{ backgroundColor: "#00796b" }}></span>
            Winch Launch
          </div>
          <div className={styles.legendItem}>
            <span className={styles.legendColor} style={{ backgroundColor: "#2e7d32" }}></span>
            Hang Launch
          </div>
          <div className={styles.legendItem}>
            <span className={styles.legendColor} style={{ backgroundColor: "#c62828" }}></span>
            Landing
          </div>
        </>
      ) : (
        <>
          <div className={styles.legendItem}>
            <span className={styles.legendColor} style={{ backgroundColor: "#00796b" }}></span>
            Winch
          </div>
          <div className={styles.legendItem}>
            <span className={styles.legendColor} style={{ backgroundColor: "#2e7d32" }}></span>
            Hang
          </div>
          <div className={styles.legendItem}>
            <span className={styles.legendColor} style={{ backgroundColor: "#7b1fa2" }}></span>
            Winch + Hang
          </div>
        </>
      )}
    </div>
  );
}
