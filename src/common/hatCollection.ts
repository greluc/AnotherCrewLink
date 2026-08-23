// Shared by the renderer, which builds the image URLs, and the main process, which
// only recolours images from this exact origin.
//
// Pinned to a commit rather than a branch. jsDelivr serves whatever the branch holds
// at request time, from a third-party account and with no integrity check, so the
// hats every user downloads could otherwise change without a release on our side.
export const HAT_COLLECTION_COMMIT = '3d2cc7de7b193618d6247a6604748a0eb56e6f3a';
export const HAT_COLLECTION_URL = `https://cdn.jsdelivr.net/gh/OhMyGuus/BetterCrewLink-Hats@${HAT_COLLECTION_COMMIT}/`;
