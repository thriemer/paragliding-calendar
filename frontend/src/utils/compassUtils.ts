export function calculateClockwiseDiff(startDegrees: number, stopDegrees: number): number {
  return (stopDegrees - startDegrees + 360) % 360;
}

export function calculateLargeArc(startDegrees: number, stopDegrees: number): number {
  const diff = calculateClockwiseDiff(startDegrees, stopDegrees);
  return diff > 180 ? 1 : 0;
}

export function calculateSweepFlag(): number {
  return 1;
}

export function degreesToRadians(degrees: number): number {
  return (degrees - 90) * (Math.PI / 180);
}

export function radiansToDegrees(radians: number): number {
  let deg = (radians * 180 / Math.PI) + 90;
  if (deg < 0) deg += 360;
  return deg;
}
