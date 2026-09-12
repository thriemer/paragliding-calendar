import { useState } from "react";
import { PreferenceActivity, PreferencePairData } from "../hooks/usePreferences";
import { API } from "../config/api";
import { kindColor, kindLabel } from "../utils/activityKind";
import styles from "./PreferencePair.module.css";

interface Props {
  pair: PreferencePairData;
  onVote: (activityId: string) => void;
  onLikeBoth: () => void;
  onDislikeBoth: () => void;
  onSkip: () => void;
  disabled?: boolean;
}

/**
 * Cycles through an activity's gallery, one image at a time. Navigation controls
 * (arrows + dots) only appear when there is more than one image; wraps around at
 * the ends. Mounted with a per-activity `key` so the index resets when the card's
 * activity changes after a vote.
 */
function ImageCarousel({ hashes, alt }: { hashes: string[]; alt: string }) {
  const [index, setIndex] = useState(0);
  const current = hashes[index];
  if (current === undefined) return null;

  const go = (delta: number) =>
    setIndex((i) => (i + delta + hashes.length) % hashes.length);

  return (
    <div className={styles.carousel}>
      <img
        className={styles.image}
        src={API.image(current)}
        alt={alt}
        loading="lazy"
      />
      {hashes.length > 1 && (
        <>
          <button
            type="button"
            className={`${styles.navButton} ${styles.navPrev}`}
            onClick={() => go(-1)}
            aria-label="Previous image"
          >
            ‹
          </button>
          <button
            type="button"
            className={`${styles.navButton} ${styles.navNext}`}
            onClick={() => go(1)}
            aria-label="Next image"
          >
            ›
          </button>
          <div className={styles.dots}>
            {hashes.map((hash, i) => (
              <button
                type="button"
                key={`${hash}-${i}`}
                className={`${styles.dot} ${i === index ? styles.dotActive : ""}`}
                onClick={() => setIndex(i)}
                aria-label={`Go to image ${i + 1}`}
                aria-current={i === index}
              />
            ))}
          </div>
        </>
      )}
    </div>
  );
}

function ActivityCard({ activity, badge }: { activity: PreferenceActivity; badge: string }) {
  const stats = Object.entries(activity.stats);
  return (
    <article className={styles.card}>
      <header className={styles.cardHeader}>
        <span className={styles.pick}>{badge}</span>
        <span className={styles.kind} style={{ background: kindColor(activity.kind) }}>
          {kindLabel(activity.kind)}
        </span>
      </header>
      <ImageCarousel key={activity.id} hashes={activity.image_hashes ?? []} alt={activity.title} />
      <h3 className={styles.title}>{activity.title}</h3>
      {stats.length > 0 && (
        <dl className={styles.stats}>
          {stats.map(([label, value]) => (
            <div key={label} className={styles.stat}>
              <dt>{label}</dt>
              <dd>{value}</dd>
            </div>
          ))}
        </dl>
      )}
      <p className={styles.description}>{activity.description}</p>
    </article>
  );
}

export function PreferencePair({
  pair,
  onVote,
  onLikeBoth,
  onDislikeBoth,
  onSkip,
  disabled = false,
}: Props) {
  return (
    <div className={styles.pair}>
      <div className={styles.cards}>
        <ActivityCard activity={pair.a} badge="A" />
        <ActivityCard activity={pair.b} badge="B" />
      </div>
      <div className={styles.actions}>
        <button
          className="btn"
          onClick={() => onVote(pair.a.id)}
          disabled={disabled}
        >
          Prefer A
        </button>
        <button
          className="btn"
          onClick={() => onVote(pair.b.id)}
          disabled={disabled}
        >
          Prefer B
        </button>
        <button
          className="btn"
          onClick={onLikeBoth}
          disabled={disabled}
        >
          Like Both
        </button>
        <button
          className="btn btn-cancel"
          onClick={onDislikeBoth}
          disabled={disabled}
        >
          Dislike Both
        </button>
        <button
          className={`${styles.skipBtn} btn ${styles.skipBtnStyle}`}
          onClick={onSkip}
          disabled={disabled}
        >
          Skip
        </button>
      </div>
    </div>
  );
}
