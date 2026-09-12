import { usePreferences } from "../hooks/usePreferences";
import { PreferencePair } from "./PreferencePair";
import { PreferenceResults } from "./PreferenceResults";
import styles from "./PreferenceScreen.module.css";

interface Props {
  onBack: () => void;
}

export function PreferenceScreen({ onBack }: Props) {
  const prefs = usePreferences();

  return (
    <div className={styles.screen}>
      <header className={styles.header}>
        <h2>Preferences</h2>
        <span className={styles.tally}>
          {prefs.comparisonsDone} comparisons
        </span>
        <div className={styles.headerActions}>
          <button className="btn btn-back" onClick={onBack}>
            Back to Main
          </button>
        </div>
      </header>

      <main className={styles.body}>
        <section className={styles.pairSection}>
          {prefs.error ? (
            <div className={styles.error}>Failed to load comparison: {prefs.error}</div>
          ) : prefs.loading ? (
            <p>Loading comparison…</p>
          ) : prefs.pair ? (
            <PreferencePair
              pair={prefs.pair}
              onVote={prefs.vote}
              onLikeBoth={prefs.likeBoth}
              onDislikeBoth={prefs.dislikeBoth}
              onSkip={prefs.skip}
              disabled={prefs.voting}
            />
          ) : (
            <p>No activities to compare yet.</p>
          )}
        </section>

        <aside className={styles.metricsSection}>
          <PreferenceResults
            summary={prefs.summary}
            loading={prefs.summaryLoading}
            error={prefs.summaryError}
            matrix={prefs.matrix}
            matrixLoading={prefs.matrixLoading}
            matrixError={prefs.matrixError}
            onReEmbed={prefs.reEmbed}
            reEmbedding={prefs.reEmbedding}
          />
        </aside>
      </main>
    </div>
  );
}
