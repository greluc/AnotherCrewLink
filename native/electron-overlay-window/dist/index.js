"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.overlayWindow = void 0;
const events_1 = require("events");
const path_1 = require("path");
const throttle_debounce_1 = require("throttle-debounce");
const electron_1 = require("electron");
const lib = require("node-gyp-build")((0, path_1.join)(__dirname, ".."));
var EventType;
(function (EventType) {
    EventType[EventType["EVENT_ATTACH"] = 1] = "EVENT_ATTACH";
    EventType[EventType["EVENT_FOCUS"] = 2] = "EVENT_FOCUS";
    EventType[EventType["EVENT_BLUR"] = 3] = "EVENT_BLUR";
    EventType[EventType["EVENT_DETACH"] = 4] = "EVENT_DETACH";
    EventType[EventType["EVENT_FULLSCREEN"] = 5] = "EVENT_FULLSCREEN";
    EventType[EventType["EVENT_MOVERESIZE"] = 6] = "EVENT_MOVERESIZE";
})(EventType || (EventType = {}));
class OverlayWindow extends events_1.EventEmitter {
    constructor() {
        super();
        this.initialized = false;
        this.defaultBehavior = true;
        this.lastBounds = { x: 0, y: 0, width: 0, height: 0 };
        this.hidden = true;
        this.active = false;
        this.WINDOW_OPTS = {
            fullscreenable: true,
            skipTaskbar: true,
            frame: false,
            show: false,
            transparent: true,
            // let Chromium to accept any size changes from OS
            resizable: true,
            focusable: false
        };
        this.on("attach", (e) => {
            var _a;
            if (this.defaultBehavior) {
                (_a = this._overlayWindow) === null || _a === void 0 ? void 0 : _a.setIgnoreMouseEvents(true); //, {forward: true});
                // linux: important to show window first before changing fullscreen
                this.active = true;
                if (!this.hidden) {
                    this._overlayWindow.showInactive();
                }
                if (e.isFullscreen !== undefined) {
                    this._overlayWindow.setFullScreen(e.isFullscreen);
                }
                this.lastBounds = e;
                this.updateOverlayBounds();
            }
        });
        this.on("fullscreen", (e) => {
            var _a;
            if (this.defaultBehavior) {
                (_a = this._overlayWindow) === null || _a === void 0 ? void 0 : _a.setFullScreen(e.isFullscreen);
            }
        });
        this.on("detach", () => {
            var _a;
            if (this.defaultBehavior) {
                if (!this.hidden) {
                    (_a = this._overlayWindow) === null || _a === void 0 ? void 0 : _a.hide();
                    this.active = false;
                }
            }
        });
        const dispatchMoveresize = (0, throttle_debounce_1.throttle)(34 /* 30fps */, this.updateOverlayBounds.bind(this));
        this.on("moveresize", (e) => {
            this.lastBounds = e;
            dispatchMoveresize();
        });
    }
    updateOverlayBounds() {
        // this._overlayWindow?.setIgnoreMouseEvents(true, {forward: true});
        let lastBounds = this.lastBounds;
        if (lastBounds.width != 0 && lastBounds.height != 0) {
            if (process.platform === "win32") {
                lastBounds = electron_1.screen.screenToDipRect(this._overlayWindow, this.lastBounds);
            }
            this._overlayWindow.setBounds(lastBounds);
            if (process.platform === "win32") {
                // if moved to screen with different DPI, 2nd call to setBounds will correctly resize window
                // dipRect must be recalculated as well
                lastBounds = electron_1.screen.screenToDipRect(this._overlayWindow, this.lastBounds);
                this._overlayWindow.setBounds(lastBounds);
            }
        }
    }
    handler(e) {
        switch (e.type) {
            case EventType.EVENT_ATTACH:
                this.emit("attach", e);
                break;
            case EventType.EVENT_FOCUS:
                this.emit("focus", e);
                break;
            case EventType.EVENT_BLUR:
                this.emit("blur", e);
                break;
            case EventType.EVENT_DETACH:
                this.emit("detach", e);
                break;
            case EventType.EVENT_FULLSCREEN:
                this.emit("fullscreen", e);
                break;
            case EventType.EVENT_MOVERESIZE:
                this.emit("moveresize", e);
                break;
        }
    }
    activateOverlay() {
        var _a;
        if (process.platform === "win32") {
            // reason: - window lags a bit using .focus()
            //         - crashes on close if using .show()
            //         - also crashes if using .moveTop()
            lib.activateOverlay();
        }
        else {
            (_a = this._overlayWindow) === null || _a === void 0 ? void 0 : _a.focus();
        }
    }
    focusTarget() {
        lib.focusTarget();
    }
    hide() {
        var _a;
        this.hidden = true;
        (_a = this._overlayWindow) === null || _a === void 0 ? void 0 : _a.hide();
    }
    stop() {
        lib.stop();
        this._overlayWindow = undefined;
        this._targetWindowTitle = "";
        this.initialized = false;
    }
    show() {
        if (!this._targetWindowTitle || !this._overlayWindow)
            return;
        this.hidden = false;
        if (!this.initialized) {
            console.log('showing window..');
            this.initialized = true;
            lib.start(this._overlayWindow.getNativeWindowHandle(), this._targetWindowTitle, this.handler.bind(this));
        }
        if (this.active) {
            //  this._overlayWindow?.showInactive();
            this.activateOverlay();
        }
    }
    attachTo(overlayWindow, targetWindowTitle) {
        if (this._overlayWindow) {
            throw new Error("Library can be initialized only once.");
        }
        this._overlayWindow = overlayWindow;
        this._targetWindowTitle = targetWindowTitle;
    }
}
exports.overlayWindow = new OverlayWindow();
//# sourceMappingURL=index.js.map