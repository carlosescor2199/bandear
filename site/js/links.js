// The repository's GitHub addresses, in one place for the scripts. The markup repeats the
// releases page as the no-JavaScript fallback of every download link (index.html, 404.html);
// tests/downloads.test.mjs checks those copies match. If the repository moves, change them all.

export const REPOSITORY = "https://github.com/carlosescor2199/bandear";
export const RELEASES_PAGE = `${REPOSITORY}/releases/latest`;
export const RELEASE_API = "https://api.github.com/repos/carlosescor2199/bandear/releases/latest";
export const LICENSE_PAGE = `${REPOSITORY}/blob/main/LICENSE`;
