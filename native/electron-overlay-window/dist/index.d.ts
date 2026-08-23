import { EventEmitter } from "events";
import type { BrowserWindow } from "electron";
export interface AttachEvent {
    hasAccess: boolean | undefined;
    isFullscreen: boolean | undefined;
    x: number;
    y: number;
    width: number;
    height: number;
}
export interface FullscreenEvent {
    isFullscreen: boolean;
}
export interface MoveresizeEvent {
    x: number;
    y: number;
    width: number;
    height: number;
}
declare interface OverlayWindow {
    on(event: "attach", listener: (e: AttachEvent) => void): this;
    on(event: "focus", listener: () => void): this;
    on(event: "blur", listener: () => void): this;
    on(event: "detach", listener: () => void): this;
    on(event: "fullscreen", listener: (e: FullscreenEvent) => void): this;
    on(event: "moveresize", listener: (e: MoveresizeEvent) => void): this;
}
declare class OverlayWindow extends EventEmitter {
    private _overlayWindow;
    private _targetWindowTitle?;
    private initialized;
    defaultBehavior: boolean;
    private lastBounds;
    private hidden;
    private active;
    readonly WINDOW_OPTS: {
        readonly fullscreenable: true;
        readonly skipTaskbar: true;
        readonly frame: false;
        readonly show: false;
        readonly transparent: true;
        readonly resizable: true;
        readonly focusable: false;
    };
    constructor();
    private updateOverlayBounds;
    private handler;
    activateOverlay(): void;
    focusTarget(): void;
    hide(): void;
    stop(): void;
    show(): void;
    attachTo(overlayWindow: BrowserWindow, targetWindowTitle: string): void;
}
export declare const overlayWindow: OverlayWindow;
export {};
