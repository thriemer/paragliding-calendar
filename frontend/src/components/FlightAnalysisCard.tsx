import { FlightAnalysis } from "../hooks/useFlightAnalytics";
import styles from "./FlightAnalysisCard.module.css";

interface FlightAnalysisCardProps {
  analysis: FlightAnalysis;
}

interface MetricCardProps {
  label: string;
  value: string;
  variant?: "default" | "positive" | "negative";
}

function MetricCard({ label, value, variant = "default" }: MetricCardProps) {
  return (
    <div className={`${styles.card} ${styles[variant]}`}>
      <div className={styles.label}>{label}</div>
      <div className={styles.value}>{value}</div>
    </div>
  );
}

export function FlightAnalysisCard({ analysis }: FlightAnalysisCardProps) {
  const parseDuration = (duration: string): string => {
    const match = duration.match(/(\d+)h\s*(\d+)m\s*(\d+\.\d+)s/);
    if (match) {
      const [, hours, minutes, seconds] = match;
      const parts = [];
      if (parseInt(hours) > 0) parts.push(`${hours}h`);
      if (parseInt(minutes) > 0) parts.push(`${minutes}m`);
      parts.push(`${seconds}s`);
      return parts.join(" ");
    }
    return duration;
  };

  const formatGlide = (glide: number): string => {
    if (!isFinite(glide)) return "∞";
    return glide.toFixed(1);
  };

  return (
    <div className={styles.container}>
      <div className={styles.title}>Flight Analysis</div>
      <div className={styles.grid}>
        <MetricCard label="Duration" value={parseDuration(analysis.duration)} />
        <MetricCard label="Distance" value={analysis.distance} />
        <MetricCard label="Max Altitude" value={analysis.max_altitude} />
        <MetricCard label="Track Length" value={analysis.track_length} />
        <MetricCard label="Max Climb" value={analysis.max_climb} variant="positive" />
        <MetricCard label="Max Sink" value={analysis.max_sink} variant="negative" />
        <MetricCard label="Min Speed" value={analysis.min_speed} />
        <MetricCard label="Max Speed" value={analysis.max_speed} />
        <MetricCard label="Min Glide" value={`${formatGlide(analysis.min_glide)}:1`} />
        <MetricCard label="Avg Glide" value={`${formatGlide(analysis.avg_glide)}:1`} />
        <MetricCard label="Elevation Gain" value={analysis.total_elevation_gain} />
      </div>
    </div>
  );
}