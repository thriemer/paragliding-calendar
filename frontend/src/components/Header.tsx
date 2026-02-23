import { FC } from "react";

interface HeaderProps {
  onLoad: () => void;
  onSave: () => void;
  saving: boolean;
  onBack?: () => void;
}

export const Header: FC<HeaderProps> = ({ onLoad, onSave, saving, onBack }) => {
  return (
    <header className="header">
      <div className="header-left">
        {onBack && (
          <button onClick={onBack} className="btn btn-back">
            ← Back
          </button>
        )}
        <h2>Flyable Decision Rule Editor</h2>
      </div>
      <div className="header-actions">
        <button onClick={onLoad} className="btn" disabled={saving}>
          Load
        </button>
        <button onClick={onSave} className="btn" disabled={saving}>
          {saving ? "Saving..." : "Save"}
        </button>
      </div>
    </header>
  );
};
