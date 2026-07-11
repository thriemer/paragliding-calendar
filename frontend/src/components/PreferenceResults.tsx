import { useState } from "react";
import { ComparisonMatrix, PreferenceSummary } from "../hooks/usePreferences";
import { kindColor, kindLabel } from "../utils/activityKind";
import styles from "./PreferenceResults.module.css";

interface Props {
  summary: PreferenceSummary | null;
  loading: boolean;
  error: string | null;
  matrix: ComparisonMatrix | null;
  matrixLoading: boolean;
  matrixError: string | null;
  onReEmbed: () => void;
  reEmbedding: boolean;
}

function cellColor(count: number): string {
  if (count >= 5) return "#c8e6c9";
  if (count >= 1) return "#fff9c4";
  return "#ffcdd2";
}

function ComparisonMatrixTable({ matrix }: { matrix: ComparisonMatrix }) {
  const allKinds = [
    ...new Set([
      ...Object.keys(matrix),
      ...Object.values(matrix).flatMap(Object.keys),
    ]),
  ].sort();

  if (allKinds.length === 0) return <p>No comparisons yet.</p>;

  return (
    <div className={styles.matrixWrap}>
      <table className={styles.matrix}>
        <thead>
          <tr>
            <th></th>
            {allKinds.map((k) => (
              <th key={k} className={styles.matrixHeader}>
                {kindLabel(k)}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {allKinds.map((winner) => (
            <tr key={winner}>
              <th className={styles.matrixHeader}>{kindLabel(winner)}</th>
              {allKinds.map((loser) => {
                const count = matrix[winner]?.[loser] ?? 0;
                return (
                  <td
                    key={loser}
                    className={styles.matrixCell}
                    style={{ background: cellColor(count) }}
                  >
                    {count}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function PreferenceResults({
  summary,
  loading,
  error,
  matrix,
  matrixLoading,
  matrixError,
  onReEmbed,
  reEmbedding,
}: Props) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  if (error) return <div className={styles.error}>Failed to load results: {error}</div>;
  if (matrixError) return <div className={styles.error}>Failed to load matrix: {matrixError}</div>;

  const toggleKind = (kind: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(kind)) next.delete(kind);
      else next.add(kind);
      return next;
    });
  };

  const kinds = summary?.kinds
    ? Object.entries(summary.kinds).sort(([, a], [, b]) => b.base_pref - a.base_pref)
    : [];

  return (
    <div className={styles.results}>
      <section>
        <h3 className={styles.heading}>Preferred activity kinds</h3>
        {kinds.length === 0 ? (
          <p className={styles.empty}>No preference data yet.</p>
        ) : (
          <ul className={styles.kindList}>
            {kinds.map(([kind, info]) => {
              const isExpanded = expanded.has(kind);
              const hasFeatures = info.features != null && info.features.length > 0;
              return (
                <li key={kind}>
                  <button
                    className={`${styles.kindRow} ${isExpanded ? styles.expanded : ""}`}
                    onClick={() => toggleKind(kind)}
                    disabled={!hasFeatures}
                  >
                    <span className={styles.kindBadge} style={{ background: kindColor(kind) }}>
                      {kindLabel(kind)}
                    </span>
                    <span className={styles.kindScore}>{info.base_pref.toFixed(2)}</span>
                    <span className={styles.kindCount}>{info.activity_count} activities</span>
                    {hasFeatures && (
                      <span className={styles.chevron}>{isExpanded ? "▲" : "▼"}</span>
                    )}
                  </button>
                  {isExpanded && hasFeatures && (
                    <ul className={styles.kindFeatures}>
                      {info.features.map((f) => (
                        <li key={f.feature} className={styles.featureRow}>
                          <span className={styles.featureName}>{f.feature}</span>
                          <span className={styles.featureWeight}>{f.weight.toFixed(2)}</span>
                          <span className={styles.featureDirection}>{f.direction}</span>
                        </li>
                      ))}
                    </ul>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <section>
        <h3 className={styles.heading}>Comparison matrix</h3>
        {matrixLoading && !matrix ? (
          <p className={styles.empty}>Loading…</p>
        ) : matrix ? (
          <ComparisonMatrixTable matrix={matrix} />
        ) : (
          <p className={styles.empty}>No comparisons yet.</p>
        )}
      </section>

      <section>
        <button className="btn" onClick={onReEmbed} disabled={reEmbedding}>
          {reEmbedding ? "Re-embedding…" : "Re-embed activities"}
        </button>
      </section>
    </div>
  );
}