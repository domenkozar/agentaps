const releaseUrl = 'https://api.github.com/repos/domenkozar/agentaps/releases/latest';
const patterns = {
  linux: /(?:linux|unknown-linux|appimage|\.deb$|\.rpm$)/i,
  macos: /(?:macos|darwin|\.dmg$|\.pkg$)/i,
  windows: /(?:windows|pc-windows|\.msi$|\.exe$)/i,
};
const platformNames = { linux: 'Linux', macos: 'macOS', windows: 'Windows' };
const installerFile = /\.(?:AppImage|deb|rpm|tar\.gz|tar\.xz|zip|dmg|pkg|msi|exe)$/i;

fetch(releaseUrl, { headers: { Accept: 'application/vnd.github+json' } })
  .then((response) => response.ok ? response.json() : null)
  .then((release) => {
    if (!release || !Array.isArray(release.assets)) return;
    for (const [platform, pattern] of Object.entries(patterns)) {
      const slot = document.querySelector(`.platform-download[data-platform="${platform}"]`);
      if (!slot) continue;
      const assets = release.assets.filter((asset) => pattern.test(asset.name) && installerFile.test(asset.name) && asset.browser_download_url);
      if (assets.length === 0) continue;
      slot.replaceChildren(...assets.map((asset) => {
        const link = document.createElement('a');
        link.className = 'platform-link';
        link.href = asset.browser_download_url;
        link.textContent = assets.length === 1 ? `Download ${platformNames[platform]}` : asset.name;
        return link;
      }));
    }
  })
  .catch(() => {});
