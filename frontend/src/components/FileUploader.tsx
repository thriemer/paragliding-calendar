import { useSiteImport } from "../hooks/useSiteImport";
import { useFileDragDrop } from "../hooks/useFileDragDrop";
import "./FileUploader.css";

interface FileUploaderProps {
  onImport: () => void;
}

export function FileUploader({ onImport }: FileUploaderProps) {
  const { importSites, importing, result, error } = useSiteImport();
  const { isDragging, dropZoneProps, fileInputProps } = useFileDragDrop(async (file) => {
    const importResult = await importSites(file);
    if (importResult) onImport();
  });

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
