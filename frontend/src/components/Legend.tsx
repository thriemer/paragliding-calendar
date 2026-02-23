interface LegendProps {
  isZoomedIn: boolean;
}

export function Legend({ isZoomedIn }: LegendProps) {
  return (
    <div className="map-legend">
      {isZoomedIn ? (
        <>
          <div className="legend-item">
            <span className="legend-color" style={{ backgroundColor: "#00796b" }}></span>
            Winch Launch
          </div>
          <div className="legend-item">
            <span className="legend-color" style={{ backgroundColor: "#2e7d32" }}></span>
            Hang Launch
          </div>
          <div className="legend-item">
            <span className="legend-color" style={{ backgroundColor: "#c62828" }}></span>
            Landing
          </div>
        </>
      ) : (
        <>
          <div className="legend-item">
            <span className="legend-color" style={{ backgroundColor: "#00796b" }}></span>
            Winch
          </div>
          <div className="legend-item">
            <span className="legend-color" style={{ backgroundColor: "#2e7d32" }}></span>
            Hang
          </div>
          <div className="legend-item">
            <span className="legend-color" style={{ backgroundColor: "#7b1fa2" }}></span>
            Winch + Hang
          </div>
        </>
      )}
    </div>
  );
}
