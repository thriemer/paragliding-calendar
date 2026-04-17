import { useState, useRef } from "react";
import { useFlightAnalytics } from "../hooks/useFlightAnalytics";
import { FlightMap } from "./FlightMap";
import { FlightAnalysisCard } from "./FlightAnalysisCard";
import styles from "./FlightUploader.module.css";

export function FlightUploader() {
  const [isDragging, setIsDragging] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const { analyzeFlight, analyzing, analysis, error, clearAnalysis } = useFlightAnalytics();

  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(true);
  };

  const handleDragLeave = () => {
    setIsDragging(false);
  };

  const handleDrop = async (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(false);

    const files = e.dataTransfer.files;
    if (files.length > 0) {
      const file = files[0];
      await analyzeFlight(file);
    }
  };

  const handleFileSelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (files && files.length > 0) {
      const file = files[0];
      await analyzeFlight(file);
    }
    if (fileInputRef.current) {
      fileInputRef.current.value = "";
    }
  };

  const handleClick = () => {
    fileInputRef.current?.click();
  };

  return (
    <div className={styles.container}>
      <div
        className={`${styles.dropZone} ${isDragging ? styles.dragging : ""} ${analyzing ? styles.uploading : ""}`}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
        onClick={handleClick}
      >
        <input
          ref={fileInputRef}
          type="file"
          accept=".kml"
          onChange={handleFileSelect}
          style={{ display: "none" }}
        />
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