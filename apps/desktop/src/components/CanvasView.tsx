import { useCallback, useEffect, useRef, useState } from "react";
import { api, decodeFrame } from "../api";
import type { PointerKind } from "../bindings/PointerKind";
import { refreshSnapshot } from "../actions";
import { store, useUi } from "../store";

/**
 * Hosts the GPU-rendered canvas. All drawing, picking and snapping happen in Rust; this component
 * only forwards pointer input and paints the frames it gets back.
 */
export function CanvasView() {
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });
  const [error, setError] = useState<string | null>(null);
  const tick = useUi((s) => s.renderTick);
  const mode = useUi((s) => s.snapshot?.mode);
  const rendering = useRef(false);
  const again = useRef(false);
  const spaceDown = useRef(false);
  const moveInFlight = useRef(false);
  const pendingMove = useRef<PointerEvent | null>(null);

  const draw = useCallback(async () => {
    const canvas = canvasRef.current;
    if (!canvas || size.w < 2 || size.h < 2) return;
    if (rendering.current) {
      again.current = true;
      return;
    }
    rendering.current = true;
    try {
      do {
        again.current = false;
        const frame = decodeFrame(await api.renderFrame(size.w, size.h));
        if (canvas.width !== frame.width || canvas.height !== frame.height) {
          canvas.width = frame.width;
          canvas.height = frame.height;
        }
        canvas
          .getContext("2d")
          ?.putImageData(new ImageData(frame.pixels, frame.width, frame.height), 0, 0);
        setError(null);
      } while (again.current);
    } catch (e) {
      setError(String(e));
    } finally {
      rendering.current = false;
    }
  }, [size]);

  useEffect(() => {
    void draw();
  }, [draw, tick, mode]);

  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      const dpr = window.devicePixelRatio || 1;
      const r = el.getBoundingClientRect();
      setSize({
        w: Math.max(1, Math.round(r.width * dpr)),
        h: Math.max(1, Math.round(r.height * dpr)),
      });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  /** Canvas-space (device pixel) coordinates of a DOM pointer/wheel event. */
  const toCanvas = (e: { clientX: number; clientY: number }) => {
    const c = canvasRef.current;
    if (!c) return { x: 0, y: 0 };
    const r = c.getBoundingClientRect();
    return {
      x: ((e.clientX - r.left) / r.width) * size.w,
      y: ((e.clientY - r.top) / r.height) * size.h,
    };
  };

  const send = useCallback(
    async (
      kind: PointerKind,
      e: {
        clientX: number;
        clientY: number;
        button?: number;
        shiftKey: boolean;
        ctrlKey: boolean;
        altKey: boolean;
        metaKey: boolean;
      },
      delta = 0,
    ) => {
      const { x, y } = toCanvas(e);
      let button = e.button ?? 0;
      if (spaceDown.current && button === 0) button = 1; // Space + drag pans
      try {
        const res = await api.pointer({
          kind,
          x,
          y,
          button,
          shift: e.shiftKey,
          ctrl: e.ctrlKey || e.metaKey,
          alt: e.altKey,
          delta,
        });
        store.set({ pointer: res, hint: res.hint });
        if (res.selection_changed || res.model_changed) await refreshSnapshot();
        else if (res.redraw) void draw();
      } catch (err) {
        setError(String(err));
      }
    },
    [size, draw],
  );

  // Wheel needs a non-passive listener to prevent page scrolling.
  useEffect(() => {
    const c = canvasRef.current;
    if (!c) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const unit = e.deltaMode === 1 ? 33 : e.deltaMode === 2 ? 400 : 1;
      void send("wheel", e, e.deltaY * unit);
    };
    c.addEventListener("wheel", onWheel, { passive: false });
    return () => c.removeEventListener("wheel", onWheel);
  }, [send]);

  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (
        e.code === "Space" &&
        !(e.target instanceof HTMLInputElement) &&
        !(e.target instanceof HTMLTextAreaElement)
      )
        spaceDown.current = true;
    };
    const up = (e: KeyboardEvent) => {
      if (e.code === "Space") spaceDown.current = false;
    };
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
    };
  }, []);

  // Moves are coalesced: at most one in flight, always the latest position.
  const onMove = (e: React.PointerEvent) => {
    pendingMove.current = e.nativeEvent;
    if (moveInFlight.current) return;
    moveInFlight.current = true;
    const pump = async () => {
      while (pendingMove.current) {
        const ev = pendingMove.current;
        pendingMove.current = null;
        await send("move", ev);
      }
      moveInFlight.current = false;
    };
    void pump();
  };

  return (
    <div className="canvas-wrap" ref={wrapRef}>
      <canvas
        ref={canvasRef}
        className="canvas"
        tabIndex={0}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId);
          e.currentTarget.focus();
          store.set({ contextMenu: null });
          void send("down", e.nativeEvent);
        }}
        onPointerMove={onMove}
        onPointerUp={(e) => {
          e.currentTarget.releasePointerCapture(e.pointerId);
          void send("up", e.nativeEvent);
        }}
        onPointerLeave={(e) => void send("leave", e.nativeEvent)}
        onDoubleClick={(e) => void send("double_click", e.nativeEvent)}
        onContextMenu={(e) => {
          e.preventDefault();
          store.set({ contextMenu: { x: e.clientX, y: e.clientY } });
        }}
      />
      {error && (
        <div className="canvas-error" role="alert">
          <strong>Canvas unavailable</strong>
          <span>{error}</span>
        </div>
      )}
    </div>
  );
}
