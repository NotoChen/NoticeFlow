import { useEffect } from "react";

export function useAutoDismissNotice(notice: string, setNotice: (message: string) => void) {
  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(""), 2200);
    return () => window.clearTimeout(timer);
  }, [notice, setNotice]);
}
