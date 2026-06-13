import { describe, test, expect } from "vitest";
import { API } from "./api";

describe("API URL builder", () => {
  test("constructs settings endpoint", () => {
    expect(API.settings).toBe("/api/settings");
  });

  test("constructs sites endpoint", () => {
    expect(API.sites).toBe("/api/sites");
  });

  test("constructs siteImport endpoint", () => {
    expect(API.siteImport).toBe("/api/sites/import");
  });

  test("constructs weatherModels endpoint", () => {
    expect(API.weatherModels).toBe("/api/weather-models");
  });

  test("constructs flightAnalyze endpoint", () => {
    expect(API.flightAnalyze).toBe("/api/flights/analyze");
  });

  test("siteDelete encodes special characters in name", () => {
    expect(API.siteDelete("My Site / Test")).toBe(
      "/api/sites/My%20Site%20%2F%20Test",
    );
  });

  test("siteDelete handles unicode names", () => {
    expect(API.siteDelete("Größer/Östlich")).toContain("/api/sites/");
    expect(API.siteDelete("Größer/Östlich")).toBe(
      `/api/sites/${encodeURIComponent("Größer/Östlich")}`,
    );
  });

  test("elevation embeds latitude and longitude", () => {
    expect(API.elevation(47.5, 10.25)).toBe(
      "/api/elevation?latitude=47.5&longitude=10.25",
    );
  });

  test("elevation handles negative coordinates", () => {
    expect(API.elevation(-33.86, -70.66)).toBe(
      "/api/elevation?latitude=-33.86&longitude=-70.66",
    );
  });
});
