// Shared between the main and renderer process. These used to live in
// main/avatarGenerator.ts and renderer/cosmetics.ts respectively, which meant the
// renderer imported jimp and fs through the main process module, and the main
// process pulled renderer image assets into its bundle.

export const DEFAULT_PLAYERCOLORS = [
	['#C51111', '#7A0838'],
	['#132ED1', '#09158E'],
	['#117F2D', '#0A4D2E'],
	['#ED54BA', '#AB2BAD'],
	['#EF7D0D', '#B33E15'],
	['#F5F557', '#C38823'],
	['#3F474E', '#1E1F26'],
	['#FFFFFF', '#8394BF'],
	['#6B2FBB', '#3B177C'],
	['#71491E', '#5E2615'],
	['#38FEDC', '#24A8BE'],
	['#50EF39', '#15A742'],
];

export const RainbowColorId = -99234;
