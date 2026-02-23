import { JSDOM } from "jsdom";

const dom = new JSDOM("<!DOCTYPE html><html><body></body></html>", {
  url: "http://localhost",
});

global.window = dom.window as any;
global.document = dom.window.document as any;
global.navigator = dom.window.navigator as any;
global.Element = dom.window.Element as any;
global.HTMLElement = dom.window.HTMLElement as any;
global.SVGElement = dom.window.SVGElement as any;
global.getComputedStyle = dom.window.getComputedStyle as any;
global.MouseEvent = dom.window.MouseEvent as any;
global.Event = dom.window.Event as any;
