import { describe, expect, it } from 'vitest';
import { type Rect, isVisibleOnSomeDisplay, overlaps } from './windowOverlap';

// The case this exists for: a saved window position from a monitor that is no longer
// connected. Without the check the app opens at coordinates nothing draws, and from the
// user's side it did not start.

const primary: Rect = { x: 0, y: 0, width: 1920, height: 1080 };
const secondary: Rect = { x: 1920, y: 0, width: 1920, height: 1080 };
const above: Rect = { x: 0, y: -1080, width: 1920, height: 1080 };

describe('overlaps', () => {
	it('is true when one rectangle is inside the other', () => {
		expect(overlaps({ x: 100, y: 100, width: 800, height: 600 }, primary)).toBe(true);
	});

	it('is true when they only partly overlap', () => {
		expect(overlaps({ x: 1820, y: 0, width: 800, height: 600 }, primary)).toBe(true);
	});

	it('is false when they only touch along an edge', () => {
		// A window whose left edge is exactly the monitor's right edge has nothing on it.
		expect(overlaps({ x: 1920, y: 0, width: 800, height: 600 }, primary)).toBe(false);
		expect(overlaps({ x: 0, y: 1080, width: 800, height: 600 }, primary)).toBe(false);
	});

	it('is false when they miss entirely', () => {
		expect(overlaps({ x: 5000, y: 5000, width: 800, height: 600 }, primary)).toBe(false);
	});

	it('is symmetric', () => {
		// It is asked as "is the window on the display", but nothing about it should
		// depend on which way round the two are passed.
		const window = { x: 1820, y: 0, width: 800, height: 600 };
		expect(overlaps(window, primary)).toBe(overlaps(primary, window));
		expect(overlaps({ x: 5000, y: 0, width: 10, height: 10 }, primary)).toBe(
			overlaps(primary, { x: 5000, y: 0, width: 10, height: 10 })
		);
	});

	it('counts a zero-area rectangle as overlapping, which is the shipped behaviour', () => {
		// Pinned rather than fixed. With strict inequalities a degenerate interval still
		// satisfies both sides — `100 < 1920` and `100 > 0` — so a saved window of zero
		// width reads as visible and is restored as a window nobody can see or grab.
		//
		// It is a hole, and a narrow one: bounds come from a real window, and the legacy
		// reader only requires that width and height are numbers. Closing it means
		// requiring positive area, which is a change to what ships and belongs to whoever
		// decides that rather than to a port's test.
		expect(overlaps({ x: 100, y: 100, width: 0, height: 600 }, primary)).toBe(true);
		expect(overlaps({ x: 100, y: 100, width: 800, height: 0 }, primary)).toBe(true);
	});
});

describe('isVisibleOnSomeDisplay', () => {
	it('accepts a window on the second monitor', () => {
		expect(isVisibleOnSomeDisplay({ x: 2000, y: 100, width: 800, height: 600 }, [primary, secondary])).toBe(true);
	});

	it('rejects the same window once that monitor is gone', () => {
		// The whole reason for the check. Unplug the second display and the saved
		// position points at nothing.
		expect(isVisibleOnSomeDisplay({ x: 2000, y: 100, width: 800, height: 600 }, [primary])).toBe(false);
	});

	it('handles a display above the primary, where coordinates are negative', () => {
		// A monitor arranged above the primary has negative y. A check written with
		// unsigned assumptions puts those windows back at the default.
		expect(isVisibleOnSomeDisplay({ x: 100, y: -900, width: 800, height: 600 }, [primary, above])).toBe(true);
	});

	it('rejects everything when no display is connected', () => {
		expect(isVisibleOnSomeDisplay({ x: 0, y: 0, width: 800, height: 600 }, [])).toBe(false);
	});

	it('accepts a window that is only just on screen', () => {
		// Deliberate, and pinned so that changing it is a decision. One pixel counts,
		// which means a window parked almost entirely off-screen is restored where it
		// was, with no title bar to grab. A stricter rule would relocate windows that
		// somebody put where they wanted them, and this function's job is to catch the
		// monitor that is gone.
		expect(isVisibleOnSomeDisplay({ x: 1919, y: 1079, width: 800, height: 600 }, [primary])).toBe(true);
	});
});
