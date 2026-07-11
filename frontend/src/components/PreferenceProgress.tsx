import styles from "./PreferenceProgress.module.css";

interface Props {
  done: number;
  total: number;
}

export function PreferenceProgress({ done, total }: Props) {
  const pct = total > 0 ? Math.min(100, (done / total) * 100) : 0;
  return (
    <div className={styles.progress}>
      <div
        className={styles.bar}
        role="progressbar"
        aria-valuenow={done}
        aria-valuemin={0}
        aria-valuemax={total}
      >
        <div className={styles.fill} style={{ width: `${pct}%` }} />
      </div>
      <span className={styles.label}>
        {done} / {total}
      </span>
    </div>
  );
}
