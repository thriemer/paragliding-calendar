import { describe, test, expect } from "bun:test";
import {
  calculateClockwiseDiff,
  calculateLargeArc,
  degreesToRadians,
  radiansToDegrees,
} from "./compassUtils";

describe("compassUtils", () => {
  describe("calculateClockwiseDiff", () => {
    test("normal case: 0 to 90 degrees", () => {
      expect(calculateClockwiseDiff(0, 90)).toBe(90);
    });

    test("normal case: 0 to 180 degrees", () => {
      expect(calculateClockwiseDiff(0, 180)).toBe(180);
    });

    test("normal case: 0 to 200 degrees (large arc)", () => {
      expect(calculateClockwiseDiff(0, 200)).toBe(200);
    });

    test("wrap around: 359 to 1 degrees (crossing north)", () => {
      expect(calculateClockwiseDiff(359, 1)).toBe(2);
    });

    test("wrap around: 270 to 90 degrees", () => {
      expect(calculateClockwiseDiff(270, 90)).toBe(180);
    });

    test("wrap around: 270 to 180 degrees", () => {
      expect(calculateClockwiseDiff(270, 180)).toBe(270);
    });

    test("wrap around: 180 to 0 degrees", () => {
      expect(calculateClockwiseDiff(180, 0)).toBe(180);
    });

    test("wrap around: 90 to 270 degrees", () => {
      expect(calculateClockwiseDiff(90, 270)).toBe(180);
    });

    test("same angle: 90 to 90 degrees", () => {
      expect(calculateClockwiseDiff(90, 90)).toBe(0);
    });

    test("full circle: 0 to 359 degrees", () => {
      expect(calculateClockwiseDiff(0, 359)).toBe(359);
    });

    test("wrap around: 350 to 10 degrees", () => {
      expect(calculateClockwiseDiff(350, 10)).toBe(20);
    });

    test("wrap around: 180 to 270 degrees", () => {
      expect(calculateClockwiseDiff(180, 270)).toBe(90);
    });
  });

  describe("calculateLargeArc", () => {
    test("small arc: 0 to 90 degrees", () => {
      expect(calculateLargeArc(0, 90)).toBe(0);
    });

    test("exactly 180 degrees: 0 to 180 degrees", () => {
      expect(calculateLargeArc(0, 180)).toBe(0);
    });

    test("large arc: 0 to 200 degrees", () => {
      expect(calculateLargeArc(0, 200)).toBe(1);
    });

    test("wrap small arc: 359 to 1 degrees", () => {
      expect(calculateLargeArc(359, 1)).toBe(0);
    });

    test("wrap exactly 180: 270 to 90 degrees", () => {
      expect(calculateLargeArc(270, 90)).toBe(0);
    });

    test("wrap large arc: 270 to 180 degrees", () => {
      expect(calculateLargeArc(270, 180)).toBe(1);
    });

    test("wrap exactly 180: 180 to 0 degrees", () => {
      expect(calculateLargeArc(180, 0)).toBe(0);
    });

    test("same angle: 90 to 90 degrees", () => {
      expect(calculateLargeArc(90, 90)).toBe(0);
    });

    test("full circle: 0 to 359 degrees", () => {
      expect(calculateLargeArc(0, 359)).toBe(1);
    });

    test("wrap small: 350 to 10 degrees", () => {
      expect(calculateLargeArc(350, 10)).toBe(0);
    });

    test("wrap small: 180 to 270 degrees", () => {
      expect(calculateLargeArc(180, 270)).toBe(0);
    });
  });

  describe("degreesToRadians", () => {
    test("0 degrees", () => {
      expect(degreesToRadians(0)).toBeCloseTo(-Math.PI / 2);
    });

    test("90 degrees", () => {
      expect(degreesToRadians(90)).toBeCloseTo(0);
    });

    test("180 degrees", () => {
      expect(degreesToRadians(180)).toBeCloseTo(Math.PI / 2);
    });

    test("270 degrees", () => {
      expect(degreesToRadians(270)).toBeCloseTo(Math.PI);
    });

    test("360 degrees", () => {
      expect(degreesToRadians(360)).toBeCloseTo(3 * Math.PI / 2);
    });
  });

  describe("radiansToDegrees", () => {
    test("-PI/2 radians (0 degrees)", () => {
      expect(radiansToDegrees(-Math.PI / 2)).toBeCloseTo(0);
    });

    test("0 radians (90 degrees)", () => {
      expect(radiansToDegrees(0)).toBeCloseTo(90);
    });

    test("PI/2 radians (180 degrees)", () => {
      expect(radiansToDegrees(Math.PI / 2)).toBeCloseTo(180);
    });

    test("PI radians (270 degrees)", () => {
      expect(radiansToDegrees(Math.PI)).toBeCloseTo(270);
    });

    test("3*PI/2 radians (360 degrees wraps to 0)", () => {
      // 3*PI/2 = 270°, which when converted becomes 360°, which is equivalent to 0°
      const result = radiansToDegrees(3 * Math.PI / 2);
      // Either 0 or 360 is acceptable as they represent the same angle
      expect(result === 0 || result === 360).toBe(true);
    });
  });
});
