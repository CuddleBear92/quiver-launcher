/**
 * Settings › Cache (cache.ts): whether the download cache keeps anything and what, how much room it has, where it is,
 * what's in it now, and, for the modes that download by themselves, how that's going.
 */
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { Archive, Ban, Download, FolderOpen, HardDrive, Layers, ListChecks, TriangleAlert, X } from "lucide-react";
import { plural, t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { i18n } from "@quiverlauncher/i18n";
import { Field } from "@quiverlauncher/ui/controls";
import { CACHE_MODES, LIMITS_GB, cacheOf, fetches, isSelected, protectedFiles, withSelected, type CacheMode, type CacheSettings } from "./cache";
import { isOwn } from "./custom";
import { fetchNow, useFetchState } from "./cache-fetch";
import { sizeLabel } from "./downloads";
import { native, type CacheStatus } from "./native";
import { useLauncher } from "./store";

const ICONS: Record<CacheMode, ReactNode> = {
  none: <Ban size={22} aria-hidden="true" />,
  installed: <HardDrive size={22} aria-hidden="true" />,
  selected: <ListChecks size={22} aria-hidden="true" />,
  latest: <Layers size={22} aria-hidden="true" />,
  hoarder: <Archive size={22} aria-hidden="true" />,
};

const GB = 1024 ** 3;

/** A limit as it reads: "50 GB", "1 TB". */
function limitName(gb: number) {
  const tb = gb / 1000;
  return gb >= 1000 ? t({ message: `${tb} TB`, comment: "A storage size in terabytes" }) : t({ message: `${gb} GB`, comment: "A storage size in gigabytes" });
}

export function CacheSettingsSection() {
  const { settings, setSettings, config, installs, catalog } = useLauncher();
  const cache = cacheOf(settings.cache, config.cacheDir ?? "");
  const [status, setStatus] = useState<CacheStatus | null>(null);
  const [said, setSaid] = useState<{ text: string; error?: true } | null>(null);
  const [clearing, setClearing] = useState(false);
  const fetch = useFetchState();
  const set = (change: (c: CacheSettings) => CacheSettings) => setSettings((s) => ({ ...s, cache: change(s.cache ?? {}) }));

  const load = useCallback(() => {
    if (!cache.dir) return;
    native.cacheStatus(cache.dir).then(setStatus, () => setStatus(null));
  }, [cache.dir]);
  // Read again as files come in, and when a run ends.
  useEffect(load, [load, fetch.fetched, fetch.running]);

  const setLimit = (gb: number | null) => {
    set((c) => ({ ...c, limitGb: gb }));
    // Lowered: rotated down to it now, never the installed versions' files.
    void native.cacheTrim(cache.dir, gb === null ? null : Math.round(gb * GB), protectedFiles(installs)).then(load, load);
  };
  const choose = async () => {
    setSaid(null);
    const picked = await native.pickFolder(t`Choose a folder for the download cache`, cache.dir);
    if (!picked) return;
    set((c) => ({ ...c, dir: picked }));
    setSaid({ text: t`New files go in this folder. Files already in the old one stay there.` });
  };
  const clear = async () => {
    setClearing(false);
    await native.cacheRemove(cache.dir).then(
      () => setSaid({ text: t`The cache was cleared.` }),
      () => setSaid({ text: t`Couldn't clear the cache. Is its drive connected?`, error: true }),
    );
    load();
  };

  const used = status?.bytes ?? 0;
  const [usedLabel, limitLabel] = [sizeLabel(used), sizeLabel(cache.limit ?? 0)];
  const mode = CACHE_MODES.find((m) => m.mode === cache.mode)!;
  return (
    <section className="settings-section download-cache" aria-label={t`Download cache`}>
      <h3>
        <Trans comment="Heading in Settings: a folder of downloaded release files, kept to install again and as a backup">Download cache</Trans>
      </h3>
      <p className="muted">
        <Trans>
          Keep the files Quiver downloads, to install them again without downloading, even offline, and as a backup of what was released. Each file is checked against the catalog before it's
          installed.
        </Trans>
      </p>
      <div className="update-choices" role="radiogroup" aria-label={t`What the download cache keeps`}>
        {CACHE_MODES.map((m) => (
          <button
            key={m.mode}
            type="button"
            role="radio"
            aria-checked={cache.mode === m.mode}
            className="add-choice update-choice"
            onClick={() => set((c) => ({ ...c, mode: m.mode }))}
          >
            {ICONS[m.mode]}
            <span>
              <strong>{i18n._(m.name)}</strong>
              <small>{i18n._(m.about)}</small>
            </span>
          </button>
        ))}
      </div>
      {mode.warning && (
        <p className="cache-warning" role="note">
          <TriangleAlert size={16} aria-hidden="true" /> {i18n._(mode.warning)}
        </p>
      )}
      {cache.on && (
        <>
          {cache.mode !== "installed" && (
            <label className="toggle">
              <input type="checkbox" checked={cache.allPlatforms} onChange={(e) => set((c) => ({ ...c, allPlatforms: e.target.checked || undefined }))} />
              <Trans comment="Download cache setting: keep a release's builds for Windows, Linux, macOS and Android, not only this computer's">Keep every system's files</Trans>
            </label>
          )}
          <Field
            label={<Trans comment="Download cache setting: the most storage it uses">Storage limit</Trans>}
            hint={<Trans>When it's full, the files used longest ago make room. The files of the versions you have installed always stay.</Trans>}
          >
            <select value={cache.limitGb === null ? "unlimited" : String(cache.limitGb)} onChange={(e) => setLimit(e.target.value === "unlimited" ? null : Number(e.target.value))}>
              {(LIMITS_GB.includes(cache.limitGb) ? LIMITS_GB : [...LIMITS_GB, cache.limitGb]).map((gb) =>
                gb === null ? (
                  <option key="unlimited" value="unlimited">
                    {t({ message: "Unlimited", comment: "Download cache storage limit: none" })}
                  </option>
                ) : (
                  <option key={gb} value={gb}>
                    {limitName(gb)}
                  </option>
                ),
              )}
            </select>
          </Field>
          <div className="cache-usage">
            {cache.limit !== undefined && (
              <div className="storage-bar" role="meter" aria-label={t`Download cache used`} aria-valuemin={0} aria-valuemax={cache.limit} aria-valuenow={used}>
                <span style={{ width: `${Math.min(100, (used / cache.limit) * 100)}%` }} />
              </div>
            )}
            <p className="save-note">
              {status
                ? [
                    plural(status.files, { one: "# file", other: "# files" }),
                    cache.limit !== undefined ? t`${usedLabel} of ${limitLabel} used` : t`${usedLabel} used`,
                  ].join(" · ")
                : t`Nothing kept yet.`}
            </p>
          </div>
          {fetches(cache) && <FetchProgress />}
          {cache.mode === "selected" && <SelectedTitles apps={cache.apps} name={(id) => catalog[id]?.projectName} onRemove={(app) => set((c) => withSelected(c, app, false))} />}
        </>
      )}
      <div className="save-row cache-folder">
        <span className="save-main">
          <span className="save-path">{cache.dir}</span>
          <span className="save-note">{settings.cache?.dir ? t`Your own folder` : t`The default folder, in Quiver's data folder`}</span>
        </span>
        <button type="button" className="save-button" onClick={() => void native.cacheOpen().catch((e) => setSaid({ text: String(e), error: true }))}>
          <FolderOpen size={14} aria-hidden="true" /> <Trans>Open</Trans>
        </button>
        <button type="button" className="save-button" onClick={() => void choose()}>
          <Trans>Change…</Trans>
        </button>
        {settings.cache?.dir && (
          <button type="button" className="save-button" onClick={() => set(({ dir: _, ...c }) => c)}>
            <Trans>Use default</Trans>
          </button>
        )}
      </div>
      {Boolean(status?.files) &&
        (clearing ? (
          <div className="cache-clear">
            <span>{t`Remove every file in the download cache? Installed apps aren't affected.`}</span>
            <button type="button" className="danger" onClick={() => void clear()}>
              <Trans>Clear cache</Trans>
            </button>
            <button type="button" onClick={() => setClearing(false)}>
              <Trans>Cancel</Trans>
            </button>
          </div>
        ) : (
          <button type="button" className="text-button" onClick={() => setClearing(true)}>
            <Trans>Clear cache…</Trans>
          </button>
        ))}
      {said && <p className={said.error ? "save-said error" : "save-said"}>{said.text}</p>}
    </section>
  );
}

/** For the modes that download by themselves: what's under way, or how the last run went, and Check now or Stop. */
function FetchProgress() {
  const s = useFetchState();
  const c = s.current;
  const of = s.ofApps;
  const [name, version, looked, finished] = [c?.name ?? "", c?.version ?? "", s.apps, s.finishedAt ? new Date(s.finishedAt).toLocaleString(i18n.locale) : ""];
  const shown = c?.filename ? (c.total ? `${sizeLabel(c.received ?? 0)} / ${sizeLabel(c.total)}` : sizeLabel(c.received ?? 0)) : undefined;
  return (
    <div className="save-row cache-fetch" aria-live="polite">
      <Download size={16} aria-hidden="true" />
      <span className="save-main">
        {s.running ? (
          <>
            <span>
              {c?.filename
                ? t({ message: `Downloading ${name} ${version}`, comment: "Download cache: the app and version it's downloading by itself now" })
                : c
                  ? t({ message: `Looking at ${name}`, comment: "Download cache: the app whose releases it's reading" })
                  : t`Starting…`}
            </span>
            <span className="save-note">
              {[of ? t`App ${looked} of ${of}` : plural(s.apps, { one: "# app looked at", other: "# apps looked at" }), plural(s.fetched, { one: "# file kept", other: "# files kept" }), shown]
                .filter(Boolean)
                .join(" · ")}
            </span>
          </>
        ) : s.finishedAt ? (
          <>
            <span>{s.fetched ? plural(s.fetched, { one: "# new file kept", other: "# new files kept" }) : t`Up to date`}</span>
            <span className="save-note">
              {[
                t`Checked ${finished}`,
                s.failed ? plural(s.failed, { one: "# file couldn't be downloaded", other: "# files couldn't be downloaded" }) : "",
                s.noRoom ? plural(s.noRoom, { one: "# file didn't fit in the storage limit", other: "# files didn't fit in the storage limit" }) : "",
              ]
                .filter(Boolean)
                .join(" · ")}
            </span>
          </>
        ) : (
          <span>{t`Downloads start shortly after Quiver opens, then every 6 hours.`}</span>
        )}
      </span>
      {s.running ? (
        <button type="button" className="save-button" onClick={() => void native.cacheFetchCancel().catch(() => {})}>
          <X size={14} aria-hidden="true" /> <Trans comment="Stops the download cache's current download">Stop</Trans>
        </button>
      ) : (
        <button type="button" className="save-button" onClick={fetchNow}>
          <Trans>Check now</Trans>
        </button>
      )}
    </div>
  );
}

/** Selected titles' apps, each removable here; they're added from Manage on an app's page. */
function SelectedTitles({ apps, name, onRemove }: { apps: { id: string; slug: string }[]; name: (id: string) => string | undefined; onRemove: (app: { id: string; slug: string }) => void }) {
  if (!apps.length)
    return (
      <p className="muted">
        <Trans>No titles chosen yet. Open an app's page, then Manage, and turn on Keep in download cache.</Trans>
      </p>
    );
  return (
    <ul className="cache-titles" aria-label={t`Titles kept in the download cache`}>
      {apps.map((app) => {
        const shown = name(app.id) ?? app.slug;
        return (
          <li key={app.id} className="save-row">
            <span className="save-main">{shown}</span>
            <button type="button" className="save-button" aria-label={t`Stop keeping ${shown}`} onClick={() => onRemove(app)}>
              <X size={14} aria-hidden="true" />
            </button>
          </li>
        );
      })}
    </ul>
  );
}

/**
 * On an app's Manage tab, while the cache keeps Selected titles: whether it keeps this one, the newest release of it
 * downloaded in the background. Not for an app the player added, whose releases the catalog doesn't list.
 */
export function CacheChoice({ entry }: { entry: { id: string; slug: string } }) {
  const { settings, setSettings, config } = useLauncher();
  const cache = cacheOf(settings.cache, config.cacheDir ?? "");
  if (cache.mode !== "selected" || isOwn(entry.id) || !entry.slug) return null;
  const on = isSelected(cache, entry.id);
  return (
    <label className="toggle panel-toggle">
      <input type="checkbox" checked={on} onChange={(e) => setSettings((s) => ({ ...s, cache: withSelected(s.cache, entry, e.target.checked) }))} />
      <Trans comment="Toggle on an app's Manage tab: the download cache keeps this app's newest release">Keep in download cache</Trans>
    </label>
  );
}
