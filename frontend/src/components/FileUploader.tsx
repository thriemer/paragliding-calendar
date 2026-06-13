import { useSiteImport } from "../hooks/useSiteImport";
import { useFileDragDrop } from "../hooks/useFileDragDrop";
import "./FileUploader.css";

export function FileUploader() {
  const { importSites, importing, result, error } = useSiteImport();
  const { isDragging, dropZoneProps, fileInputProps } = useFileDragDrop(importSites);

  return (
    <div className="file-uploader">
      <div
        className={`drop-zone ${isDragging ? "dragging" : ""} ${importing ? "uploading" : ""}`}
        {...dropZoneProps}
      >
        <input {...fileInputProps} accept=".xml" />
        {importing ? (
          <span>Uploading...</span>
        ) : (
          <span>Drop DHV XML file here or click to browse</span>
        )}
      </div>
      {result && (
        <div className="upload-result success">
          Imported {result.imported} sites
        </div>
      )}
      {error && (
        <div className="upload-result error">
          {error}
        </div>
      )}
    </div>
  );
}
