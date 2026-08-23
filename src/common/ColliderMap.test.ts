import { describe, expect, it } from 'vitest';
import { MapType } from './AmongusMap';
import { poseCollide } from './ColliderMap';

// The collider paths are in their own coordinate space: a world point (x, y) sits at
// (x + 40, 40 - y) on the path. Each case below names the path coordinates it targets
// so the numbers can be checked against the map data.

describe('poseCollide', () => {
	it('finds the wall between two points on opposite sides of it', () => {
		// The Skeld starts with a vertical wall at path x 33.65, spanning path y 35.32
		// to 37.57, which is world x -6.35 and world y 2.43 to 4.68.
		const collided = poseCollide({ x: -7, y: 3.5 }, { x: -5.5, y: 3.5 }, MapType.THE_SKELD, []);
		expect(collided).toBe(true);
	});

	it('reports nothing between a point and itself', () => {
		expect(poseCollide({ x: -7, y: 3.5 }, { x: -7, y: 3.5 }, MapType.THE_SKELD, [])).toBe(false);
	});

	it('lets two points on the same side of that wall hear each other', () => {
		expect(poseCollide({ x: -7, y: 3.5 }, { x: -6.9, y: 3.5 }, MapType.THE_SKELD, [])).toBe(false);
	});

	it('mirrors the April Fools map onto the Skeld colliders', () => {
		// dlekS is the Skeld flipped, so the same wall sits at world x +6.35.
		expect(poseCollide({ x: 7, y: 3.5 }, { x: 5.5, y: 3.5 }, MapType.THE_SKELD_APRIL, [])).toBe(true);
	});

	it('lets sound through an open door and stops it at a closed one', () => {
		// Polus door 0, path 'M 51.257 48.531 V 50.205': world x 11.257, y -8.531 to -10.205.
		const a = { x: 11, y: -9.5 };
		const b = { x: 11.6, y: -9.5 };
		expect(poseCollide({ ...a }, { ...b }, MapType.POLUS, [])).toBe(false);
		expect(poseCollide({ ...a }, { ...b }, MapType.POLUS, [0])).toBe(true);
	});

	it('never blocks on a map it has no data for', () => {
		expect(poseCollide({ x: -7, y: 3.5 }, { x: -5.5, y: 3.5 }, MapType.UNKNOWN, [])).toBe(false);
	});
});
