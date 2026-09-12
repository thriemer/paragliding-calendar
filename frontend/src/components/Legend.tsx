import { kindLabel, kindColor } from "../utils/activityKind";
import styles from "./SitesMap.module.css";

const PIN = {
  winch: "#2A81CB",
  hang: "#2AAD27",
  both: "#9C2BCB",
  landing: "#CB2B3E",
  user: "#CB8427",
};

const ACTIVITY_KINDS = ["paragliding", "hiking", "biking", "running", "mountain_climbing", "kayaking"];

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
      {ACTIVITY_KINDS.map((kind) => (
        <div key={kind} className={styles.legendItem}>
          <span className={styles.legendColor} style={{ backgroundColor: kindColor(kind) }}></span>
          {kindLabel(kind)}
        </div>
      ))}
    </div>
  );
}