import styles from "./SitesMap.module.css";

interface LegendProps {
  isZoomedIn: boolean;
}

export function Legend({ isZoomedIn }: LegendProps) {
  return (
    <div className={styles.legend}>
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
