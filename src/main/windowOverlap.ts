/**
 * Whether a saved window rectangle still lands on a screen.
 *
 * Lifted out of `windowState.ts` for the reason `sortLobbies.ts` and
 * `validateServerUrl.ts` were lifted out of their components: the caller reaches
 * `screen.getAllDisplays()`, which needs Electron, and this is a pure comparison of
 * rectangles that the tests here can run under node.
 *
 * What it protects against is a window restored to a monitor that is no longer there —
 * unplugged, or moved in the display arrangement. Without the check the app opens at
 * coordinates nothing draws, and from the user's side it did not start.
 */

/** A saved window position and size. */
export interface Rect {
	x: number;
	y: number;
	width: number;
	height: number;
}

/**
 * Whether two rectangles share any area at all.
 *
 * Strict inequalities, so rectangles that only touch along an edge do not count: a window
 * whose right edge is exactly a monitor's left edge has nothing on that monitor.
 *
 * A rectangle of zero width or height still counts as overlapping, because a degenerate
 * interval satisfies both strict comparisons. That is the shipped behaviour and it is
 * kept; the test names it. It means a saved window of zero size reads as visible and is
 * restored as one nobody can grab — a narrow hole, since bounds come from a real window,
 * and closing it is a change to what ships rather than a port decision.
 */
export function overlaps(a: Rect, b: Rect): boolean {
	return a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;
}

/**
 * Whether the window would be visible on at least one of the connected displays.
 *
 * **Any overlap counts, including a single pixel.** That is the shipped behaviour and it
 * is kept rather than tightened: a stricter rule would move windows that users had
 * deliberately parked half off-screen, and this function's job is to catch the monitor
 * that is gone, not to police placement. The consequence is that a window left one pixel
 * on-screen is restored there, with no title bar to grab — rare, and less bad than
 * relocating a window somebody put where they wanted it.
 */
export function isVisibleOnSomeDisplay(window: Rect, displays: readonly Rect[]): boolean {
	return displays.some((display) => overlaps(window, display));
}
