import { useCallback, useEffect, useRef, useState } from "react";
import type { Dispatch, MutableRefObject, PointerEvent, SetStateAction } from "react";
import type { AutomationRule } from "../lib/tauri";

export type RuleDragState = {
  id: string;
  offsetX: number;
  offsetY: number;
  x: number;
  y: number;
};

type UseRuleDragOptions = {
  rulesRef: MutableRefObject<AutomationRule[]>;
  setRules: Dispatch<SetStateAction<AutomationRule[]>>;
  saveRuleOrder: (rules: AutomationRule[]) => Promise<void>;
  onSaved: () => void;
  onError: (error: unknown) => void;
};

export function useRuleDrag({
  rulesRef,
  setRules,
  saveRuleOrder,
  onSaved,
  onError,
}: UseRuleDragOptions) {
  const [dragging, setDragging] = useState<RuleDragState | null>(null);
  const draggingRef = useRef<RuleDragState | null>(null);
  const dragStartRef = useRef({ x: 0, y: 0 });
  const dragDidMoveRef = useRef(false);
  const dragDidReorderRef = useRef(false);
  const dragAnimationFrameRef = useRef<number | null>(null);
  const pendingDragPointRef = useRef<{ x: number; y: number } | null>(null);
  const suppressRuleClickRef = useRef(false);

  useEffect(() => {
    draggingRef.current = dragging;
  }, [dragging]);

  useEffect(() => {
    return () => {
      if (dragAnimationFrameRef.current !== null) {
        window.cancelAnimationFrame(dragAnimationFrameRef.current);
      }
    };
  }, []);

  const clearSuppressedRuleClick = useCallback(() => {
    window.setTimeout(() => {
      suppressRuleClickRef.current = false;
    }, 0);
  }, []);

  const applyDragPoint = useCallback((point: { x: number; y: number }) => {
    const draggingSnapshot = draggingRef.current;
    if (!draggingSnapshot) return;
    const nextDragging = { ...draggingSnapshot, x: point.x, y: point.y };
    draggingRef.current = nextDragging;
    setDragging(nextDragging);

    const target = document.elementFromPoint(point.x, point.y)?.closest("[data-rule-card-id]");
    const targetId = target?.getAttribute("data-rule-card-id");
    if (!targetId || targetId === draggingSnapshot.id) return;
    const currentRules = rulesRef.current ?? [];
    const from = currentRules.findIndex((rule) => rule.id === draggingSnapshot.id);
    const to = currentRules.findIndex((rule) => rule.id === targetId);
    if (from < 0 || to < 0 || from === to) return;
    const next = currentRules.slice();
    const [item] = next.splice(from, 1);
    next.splice(to, 0, item);
    dragDidReorderRef.current = true;
    rulesRef.current = next;
    setRules(next);
  }, [rulesRef, setRules]);

  const startDrag = useCallback((event: PointerEvent, rule: AutomationRule) => {
    if (event.button !== 0) return;
    if (dragAnimationFrameRef.current !== null) {
      window.cancelAnimationFrame(dragAnimationFrameRef.current);
      dragAnimationFrameRef.current = null;
    }
    pendingDragPointRef.current = null;
    const rect = event.currentTarget.getBoundingClientRect();
    dragStartRef.current = { x: event.clientX, y: event.clientY };
    dragDidMoveRef.current = false;
    dragDidReorderRef.current = false;
    suppressRuleClickRef.current = false;
    const nextDragging = {
      id: rule.id,
      offsetX: event.clientX - rect.left,
      offsetY: event.clientY - rect.top,
      x: event.clientX,
      y: event.clientY,
    };
    draggingRef.current = nextDragging;
    setDragging(nextDragging);
    event.currentTarget.setPointerCapture(event.pointerId);
  }, []);

  const moveDrag = useCallback((event: PointerEvent) => {
    const currentDragging = draggingRef.current;
    if (!currentDragging) return;
    const distance = Math.hypot(event.clientX - dragStartRef.current.x, event.clientY - dragStartRef.current.y);
    if (distance > 4) {
      dragDidMoveRef.current = true;
      suppressRuleClickRef.current = true;
    }
    pendingDragPointRef.current = { x: event.clientX, y: event.clientY };
    if (dragAnimationFrameRef.current === null) {
      dragAnimationFrameRef.current = window.requestAnimationFrame(() => {
        dragAnimationFrameRef.current = null;
        const point = pendingDragPointRef.current;
        pendingDragPointRef.current = null;
        if (!point) return;
        applyDragPoint(point);
      });
    }
  }, [applyDragPoint]);

  const endDrag = useCallback(async () => {
    const currentDragging = draggingRef.current;
    if (!currentDragging) return;
    const pendingPoint = pendingDragPointRef.current;
    if (dragAnimationFrameRef.current !== null) {
      window.cancelAnimationFrame(dragAnimationFrameRef.current);
      dragAnimationFrameRef.current = null;
    }
    pendingDragPointRef.current = null;
    if (pendingPoint) applyDragPoint(pendingPoint);
    const moved = dragDidMoveRef.current;
    const reordered = dragDidReorderRef.current;
    draggingRef.current = null;
    setDragging(null);
    dragDidMoveRef.current = false;
    dragDidReorderRef.current = false;
    if (!moved) return;
    if (!reordered) {
      clearSuppressedRuleClick();
      return;
    }
    try {
      await saveRuleOrder(rulesRef.current ?? []);
      onSaved();
    } catch (error) {
      onError(error);
    } finally {
      clearSuppressedRuleClick();
    }
  }, [applyDragPoint, clearSuppressedRuleClick, onError, onSaved, rulesRef, saveRuleOrder]);

  const isRuleClickSuppressed = useCallback(() => suppressRuleClickRef.current, []);

  return {
    dragging,
    startDrag,
    moveDrag,
    endDrag,
    isRuleClickSuppressed,
  };
}
