import { FC } from "react";
import styles from "./Header.module.css";

interface HeaderProps {
  onLoad: () => void;
  onSave: () => void;
  saving: boolean;
  onBack?: () => void;
}

export const Header: FC<HeaderProps> = ({ onLoad, onSave, saving, onBack }) => {
  return (
    <header className={styles.header}>
      {" "}
      <div className={styles.headerLeft}>
        {" "}
        {onBack && (
          <button onClick={onBack} className="btn btn-back">
            {" "}
            ← Back{" "}
          </button>
        )}{" "}
        <h2>Flyable Decision Rule Editor</h2>{" "}
      </div>{" "}
      <div className={styles.headerActions}>
        {" "}
        <button onClick={onLoad} className="btn" disabled={saving}>
          {" "}
          Load{" "}
        </button>{" "}
        <button onClick={onSave} className="btn" disabled={saving}>
          {" "}
          {saving ? "Saving..." : "Save"}{" "}
        </button>
      </div>
    </header>
  );
};
