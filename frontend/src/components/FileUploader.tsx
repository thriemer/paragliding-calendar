import { useState, useRef } from "react";
import { useSiteImport } from "../hooks/useSiteImport";
import "./FileUploader.css";

interface FileUploaderProps {
  onImport: () => void;
}

export function FileUploader({ onImport }: FileUploaderProps) {
  const [isDragging, setIsDragging] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const { importSites, importing, result, error } = useSiteImport();

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
      const importResult = await importSites(file);
      if (importResult) {
        onImport();
      }
    }
  };

  const handleFileSelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (files && files.length > 0) {
      const file = files[0];
      const importResult = await importSites(file);
      if (importResult) {
        onImport();
      }
    }
    if (fileInputRef.current) {
      fileInputRef.current.value = "";
    }
  };

  const handleClick = () => {
    fileInputRef.current?.click();
  };

  return (
    <div className="file-uploader">
      <div
        className={`drop-zone ${isDragging ? "dragging" : ""} ${importing ? "uploading" : ""}`}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
        onClick={handleClick}
      >
        <input
          ref={fileInputRef}
          type="file"
          accept=".xml"
          onChange={handleFileSelect}
          style={{ display: "none" }}
        />
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
