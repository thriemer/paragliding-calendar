import { FC } from "react";

interface HeaderProps {
  onLoad: () => void;
  onSave: () => void;
  saving: boolean;
}

export const Header: FC<HeaderProps> = ({ onLoad, onSave, saving }) => {
  return (
    <header className="header">
      <h2>Flyable Decision Rule Editor</h2>
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
