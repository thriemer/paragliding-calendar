import { useRef, useState } from "react";

export function useFileDragDrop(onFile: (file: File) => unknown) {
  const [isDragging, setIsDragging] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const dropZoneProps = {
    onDragOver: (e: React.DragEvent) => {
      e.preventDefault();
      setIsDragging(true);
    },
    onDragLeave: () => setIsDragging(false),
    onDrop: async (e: React.DragEvent) => {
      e.preventDefault();
      setIsDragging(false);
      const file = e.dataTransfer.files[0];
      if (file) await onFile(file);
    },
    onClick: () => fileInputRef.current?.click(),
  };

  const fileInputProps = {
    ref: fileInputRef,
    type: "file" as const,
    style: { display: "none" },
    onChange: async (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (file) await onFile(file);
      if (fileInputRef.current) fileInputRef.current.value = "";
    },
  };

  return { isDragging, dropZoneProps, fileInputProps };
}
