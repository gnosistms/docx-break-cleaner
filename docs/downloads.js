const page = {
  owner: null,
  repo: null,
  repoUrl: null,
  latestUrl: null,
};

function inferRepository() {
  const hostMatch = window.location.hostname.match(/^([^.]+)\.github\.io$/i);
  if (!hostMatch) return;

  const pathParts = window.location.pathname.split("/").filter(Boolean);
  page.owner = hostMatch[1];
  page.repo = pathParts[0] || `${page.owner}.github.io`;
  page.repoUrl = `https://github.com/${page.owner}/${page.repo}`;
  page.latestUrl = `${page.repoUrl}/releases/latest`;

  const githubLink = document.querySelector("#githubLink");
  githubLink.href = page.repoUrl;
  githubLink.hidden = false;
}

function classifyAsset(asset) {
  const name = asset.name.toLowerCase();
  if (name.endsWith(".dmg") && /(aarch64|arm64|apple|silicon)/.test(name)) return "mac";
  if (name.endsWith(".dmg")) return "mac";
  if (name.endsWith(".exe") || name.endsWith(".msi")) return "windows";
  return null;
}

function formatBytes(bytes) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "";
  const units = ["B", "KB", "MB", "GB"];
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** exponent;
  return `${value.toFixed(value >= 10 || exponent === 0 ? 0 : 1)} ${units[exponent]}`;
}

function setUnavailable(kind, fallbackUrl) {
  const card = document.querySelector(`[data-platform="${kind}"]`);
  const link = card.querySelector(".asset-link");
  const pill = card.querySelector(".status-pill");
  pill.textContent = "Coming soon";
  pill.classList.add("unavailable");
  link.textContent = "View releases →";
  link.href = fallbackUrl || "#";
  if (!fallbackUrl) link.classList.add("disabled");
}

function wireRelease(release) {
  const assetsByKind = new Map();
  for (const asset of release.assets || []) {
    const kind = classifyAsset(asset);
    if (kind && !assetsByKind.has(kind)) assetsByKind.set(kind, asset);
  }

  for (const kind of ["mac", "windows"]) {
    const asset = assetsByKind.get(kind);
    if (!asset) {
      if (kind === "windows") setUnavailable(kind, release.html_url || page.latestUrl);
      continue;
    }
    const link = document.querySelector(`.asset-link[data-kind="${kind}"]`);
    link.href = asset.browser_download_url;
    link.dataset.size = formatBytes(asset.size);
  }

  const isWindows = /Windows/i.test(navigator.userAgent);
  const preferredKind = isWindows ? "windows" : "mac";
  const preferredAsset = assetsByKind.get(preferredKind);
  const primary = document.querySelector("#primaryDownload");
  const heading = document.querySelector("#downloadHeading");
  const meta = document.querySelector("#downloadMeta");
  const platformIcon = document.querySelector("#platformIcon");

  if (preferredAsset) {
    const kind = classifyAsset(preferredAsset);
    primary.href = preferredAsset.browser_download_url;
    heading.textContent = kind === "windows" ? "Download for Windows" : "Download for macOS";
    platformIcon.textContent = kind === "windows" ? "⊞" : "⌘";
    meta.textContent = kind === "windows"
      ? `Windows 10 or newer${preferredAsset.size ? ` · ${formatBytes(preferredAsset.size)}` : ""}`
      : `Apple silicon · macOS 11 or newer${preferredAsset.size ? ` · ${formatBytes(preferredAsset.size)}` : ""}`;
  } else if (isWindows && release.html_url) {
    primary.href = release.html_url;
    primary.querySelector("span:first-child").textContent = "View release";
  }

  document.querySelector("#releaseNote").textContent = preferredAsset
    ? `${release.name || release.tag_name} · Latest release`
    : "Version 0.1.0 · macOS preview";
}

async function loadLatestRelease() {
  inferRepository();
  if (!page.owner || !page.repo) {
    document.querySelector("#releaseNote").textContent = "Version 0.1.0 · macOS preview";
    setUnavailable("windows", null);
    return;
  }

  try {
    const response = await fetch(`https://api.github.com/repos/${page.owner}/${page.repo}/releases/latest`, {
      headers: { Accept: "application/vnd.github+json" },
    });
    if (!response.ok) throw new Error(`GitHub API returned ${response.status}`);
    wireRelease(await response.json());
  } catch {
    document.querySelector("#releaseNote").textContent = "Version 0.1.0 · macOS preview";
    setUnavailable("windows", page.latestUrl);
  }
}

loadLatestRelease();
