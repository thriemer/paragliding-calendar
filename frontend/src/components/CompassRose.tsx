import { useState } from "react";
import { calculateLargeArc, degreesToRadians } from "../utils/compassUtils";

interface CompassRoseProps {
  startDegrees: number;
  stopDegrees: number;
  onChange: (start: number, stop: number) => void;
}

export function CompassRose({ startDegrees, stopDegrees, onChange }: CompassRoseProps) {
  const [dragMode, setDragMode] = useState<"start" | "stop" | null>(null);
  const radius = 60;
  const center = radius + 10;

  const degToRad = degreesToRadians;

  const radToDeg = (rad: number) => {
    let deg = (rad * 180 / Math.PI) + 90;
    if (deg < 0) deg += 360;
    return deg;
  };

  const handleMouseDown = (e: React.MouseEvent<SVGSVGElement>, mode: "start" | "stop") => {
    setDragMode(mode);
    handleMouseMove(e);
  };

  const handleMouseMove = (e: React.MouseEvent<SVGSVGElement>) => {
    if (!dragMode) return;
    
    const svg = e.currentTarget;
    const rect = svg.getBoundingClientRect();
    const x = e.clientX - rect.left - center;
    const y = e.clientY - rect.top - center;
    const angle = radToDeg(Math.atan2(y, x));
    
    if (dragMode === "start") {
      onChange(angle, stopDegrees);
    } else {
      onChange(startDegrees, angle);
    }
  };

  const handleMouseUp = () => {
    setDragMode(null);
  };

  const startX = center + radius * Math.cos(degToRad(startDegrees));
  const startY = center + radius * Math.sin(degToRad(startDegrees));
  const stopX = center + radius * Math.cos(degToRad(stopDegrees));
  const stopY = center + radius * Math.sin(degToRad(stopDegrees));

  const largeArc = calculateLargeArc(startDegrees, stopDegrees);
  const sweepFlag = 1;

  return (
    <div className="compass-rose">
      <svg 
        width={center * 2} 
        height={center * 2} 
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseUp}
        style={{ cursor: 'pointer' }}
      >
        <circle cx={center} cy={center} r={radius} fill="#f0f0f0" stroke="#ccc" />
        
        {[0, 45, 90, 135, 180, 225, 270, 315].map((deg) => {
          const rad = degToRad(deg);
          const x1 = center + (radius - 10) * Math.cos(rad);
          const y1 = center + (radius - 10) * Math.sin(rad);
          const x2 = center + radius * Math.cos(rad);
          const y2 = center + radius * Math.sin(rad);
          return (
            <line key={deg} x1={x1} y1={y1} x2={x2} y2={y2} stroke="#999" />
          );
        })}
        
        {["N", "E", "S", "W"].map((dir, i) => {
          const deg = i * 90;
          const rad = degToRad(deg);
          const x = center + (radius - 20) * Math.cos(rad);
          const y = center + (radius - 20) * Math.sin(rad);
          return (
            <text key={dir} x={x} y={y} textAnchor="middle" dominantBaseline="middle" fontSize="12" fontWeight="bold">
              {dir}
            </text>
          );
        })}

        <path
          d={`M ${center} ${center} L ${startX} ${startY} A ${radius} ${radius} 0 ${largeArc} ${sweepFlag} ${stopX} ${stopY} Z`}
          fill="rgba(76, 175, 80, 0.3)"
          stroke="#4caf50"
          strokeWidth="2"
        />

        <line x1={center} y1={center} x2={startX} y2={startY} stroke="#2196f3" strokeWidth="3" 
          onMouseDown={(e) => handleMouseDown(e, "start")}
          style={{ cursor: 'grab' }}
        />
        <circle cx={startX} cy={startY} r="6" fill="#2196f3" 
          onMouseDown={(e) => handleMouseDown(e, "start")}
          style={{ cursor: 'grab' }}
        />

        <line x1={center} y1={center} x2={stopX} y2={stopY} stroke="#f44336" strokeWidth="3"
          onMouseDown={(e) => handleMouseDown(e, "stop")}
          style={{ cursor: 'grab' }}
        />
        <circle cx={stopX} cy={stopY} r="6" fill="#f44336"
          onMouseDown={(e) => handleMouseDown(e, "stop")}
          style={{ cursor: 'grab' }}
        />
      </svg>
      <div className="compass-labels">
        <span style={{ color: '#2196f3' }}>Start: {Math.round(startDegrees)}°</span>
        <span style={{ color: '#f44336' }}>Stop: {Math.round(stopDegrees)}°</span>
      </div>
    </div>
  );
}
