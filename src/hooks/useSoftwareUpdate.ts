import { useCallback, useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

const UPDATE_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1_000;
const UPDATE_CHECK_TIMEOUT_MS = 60_000;
const UPDATE_DOWNLOAD_TIMEOUT_MS = 5 * 60_000;
const UPDATE_RETRY_DELAY_MS = 1_000;

export type SoftwareUpdateStatus = "idle" | "checking" | "latest" | "available" | "downloading" | "installing" | "error";

export type SoftwareUpdateState = {
  currentVersion: string;
  latestVersion: string;
  releaseNotes: string;
  releaseDate: string;
  lastCheckedAt: string;
  status: SoftwareUpdateStatus;
  error: string;
  downloadedBytes: number;
  totalBytes: number;
};

const initialState: SoftwareUpdateState = {
  currentVersion: "",
  latestVersion: "",
  releaseNotes: "",
  releaseDate: "",
  lastCheckedAt: "",
  status: "idle",
  error: "",
  downloadedBytes: 0,
  totalBytes: 0,
};

function delay(milliseconds: number) {
  return new Promise<void>((resolve) => window.setTimeout(resolve, milliseconds));
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export function useSoftwareUpdate() {
  const [state, setState] = useState<SoftwareUpdateState>(initialState);
  const availableUpdateRef = useRef<Update | null>(null);
  const checkInFlightRef = useRef<Promise<void> | null>(null);
  const installInFlightRef = useRef(false);

  const checkForUpdates = useCallback(() => {
    if (installInFlightRef.current) return Promise.resolve();
    if (checkInFlightRef.current) return checkInFlightRef.current;

    const operation = (async () => {
      setState((current) => ({ ...current, status: "checking", error: "" }));
      try {
        const currentVersion = await getVersion();
        setState((current) => ({ ...current, currentVersion }));
        let update: Update | null;
        try {
          update = await check({ timeout: UPDATE_CHECK_TIMEOUT_MS });
        } catch {
          await delay(UPDATE_RETRY_DELAY_MS);
          update = await check({ timeout: UPDATE_CHECK_TIMEOUT_MS });
        }

        if (availableUpdateRef.current && availableUpdateRef.current !== update) {
          await availableUpdateRef.current.close().catch(() => undefined);
        }
        availableUpdateRef.current = update;
        const checkedAt = new Date().toISOString();

        if (!update) {
          setState((current) => ({
            ...current,
            currentVersion,
            latestVersion: currentVersion,
            releaseNotes: "",
            releaseDate: "",
            lastCheckedAt: checkedAt,
            status: "latest",
            error: "",
          }));
          return;
        }

        setState((current) => ({
          ...current,
          latestVersion: update.version,
          releaseNotes: update.body?.trim() ?? "",
          releaseDate: update.date ?? "",
          lastCheckedAt: checkedAt,
          status: "available",
          error: "",
        }));
      } catch (error) {
        setState((current) => ({
          ...current,
          status: availableUpdateRef.current ? "available" : "error",
          error: errorMessage(error),
        }));
      } finally {
        checkInFlightRef.current = null;
      }
    })();

    checkInFlightRef.current = operation;
    return operation;
  }, []);

  const installUpdate = useCallback(async () => {
    if (installInFlightRef.current) return;
    const update = availableUpdateRef.current;
    if (!update) {
      setState((current) => ({ ...current, status: "error", error: "更新信息已失效，请重新检查更新" }));
      return;
    }

    installInFlightRef.current = true;
    setState((current) => ({
      ...current,
      status: "downloading",
      error: "",
      downloadedBytes: 0,
      totalBytes: 0,
    }));

    try {
      await update.downloadAndInstall(
        (event) => {
          if (event.event === "Started") {
            setState((current) => ({
              ...current,
              status: "downloading",
              downloadedBytes: 0,
              totalBytes: event.data.contentLength ?? 0,
            }));
          } else if (event.event === "Progress") {
            setState((current) => ({
              ...current,
              downloadedBytes: current.downloadedBytes + event.data.chunkLength,
            }));
          } else if (event.event === "Finished") {
            setState((current) => ({ ...current, status: "installing" }));
          }
        },
        { timeout: UPDATE_DOWNLOAD_TIMEOUT_MS },
      );
      setState((current) => ({ ...current, status: "installing" }));
      await relaunch();
    } catch (error) {
      setState((current) => ({ ...current, status: "available", error: errorMessage(error) }));
    } finally {
      installInFlightRef.current = false;
    }
  }, []);

  useEffect(() => {
    void checkForUpdates();
    const interval = window.setInterval(() => void checkForUpdates(), UPDATE_CHECK_INTERVAL_MS);
    return () => window.clearInterval(interval);
  }, [checkForUpdates]);

  return { state, checkForUpdates, installUpdate };
}
