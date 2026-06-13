import { useFlightAnalytics } from "../hooks/useFlightAnalytics";
import { useFileDragDrop } from "../hooks/useFileDragDrop";
import { FlightMap } from "./FlightMap";
import { FlightAnalysisCard } from "./FlightAnalysisCard";
import styles from "./FlightUploader.module.css";

export function FlightUploader() {
  const { analyzeFlight, analyzing, analysis, error, clearAnalysis } = useFlightAnalytics();
  const { isDragging, dropZoneProps, fileInputProps } = useFileDragDrop(analyzeFlight);

  return (
    <div className={styles.container}>
      <div
        className={`${styles.dropZone} ${isDragging ? styles.dragging : ""} ${analyzing ? styles.uploading : ""}`}
        {...dropZoneProps}
      >
        <input {...fileInputProps} accept=".kml" />
        {analyzing ? (
          <span>Analyzing flight...</span>
        ) : (
          <span>Drop KML flight file here or click to browse</span>
        )}
      </div>
      {error && (
        <div className={styles.error}>
          {error}
        </div>
      )}
      {analysis && (
        <div className={styles.results}>
          <div className={styles.resultsHeader}>
            <h3>Flight Results</h3>
            <button className={styles.clearButton} onClick={clearAnalysis}>
              Clear
            </button>
          </div>
          <FlightMap path={analysis.path} />
          <FlightAnalysisCard analysis={analysis} />
        </div>
      )}
    </div>
  );
}
