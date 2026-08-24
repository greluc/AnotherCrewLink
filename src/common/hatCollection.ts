// Shared by the renderer, which builds the image URLs, and the main process, which
// only recolours images from this exact origin.
//
// Pinned to a commit rather than a branch, and to our own fork rather than upstream.
// jsDelivr serves whatever a branch holds at request time with no integrity check, so a
// branch pin would let the artwork every user downloads change without a release on our
// side — and until 2026-08-24 that branch was in an account this project does not
// control.
//
// This fork carries the base game's cosmetics only. The four mod collections upstream
// ships — Town of Us, The Other Roles, Las Monjas and the Mira variant — went with the
// third-party artwork. A player running one of those mods sees no mod hat rather than an
// error: `getHat` misses, `getHatUrl` returns undefined, and nothing is drawn.
//
// Moving the pin means moving both lines. The commit alone points at a tree the new
// repository does not have.
export const HAT_COLLECTION_COMMIT = '14bb0cb592a23d2cee25a0c368506446abadaad8';
export const HAT_COLLECTION_URL = `https://cdn.jsdelivr.net/gh/greluc/AnotherCrewLink-Hats@${HAT_COLLECTION_COMMIT}/`;
