# Embedded Android window handoff

The intermittent blank Desktop with a healthy Android status had two different readiness checks mixed together: the daemon's cached health and the new window's actual connection. A STARTING/DEGRADED or stale cached state could skip the old-session stop, so `Start` reused Android still connected to another compositor.

The corrected sequence is a single `AttachEmbeddedDisplay(socket, owner)` operation in the daemon:

1. Acquire the same lifecycle mutex used for Start, Stop and Recover.
2. Stop the previous session, then observe the real CLI status until both session and container have stopped.
3. Assign the embedded socket and start detached Android.
4. Wait for actual session registration before releasing the lifecycle mutex; `setsid --fork` finishing does not mean Waydroid has registered yet.
5. The UI reads BootCompleted directly, applies display density and launches the currently selected app. This creates the first useful surface (the custom Home may not create one until explicitly launched).
6. It waits for a new frame, a guest surface size and a ready touch pipe before reporting the window ready. Waiting for the app surface before launching the app would deadlock startup.

The UI no longer treats cached RUNNING/BootCompleted as proof that Android reached this window. A failed connection displays an error with a reconnect action. A disappeared configured socket is an error, rather than permission to silently start on the host desktop compositor.

Closing an old window sends `DetachEmbeddedDisplay(owner)`. Only the current owner can detach; a delayed close callback cannot stop its replacement. Recovery also checks the startup generation under the lifecycle mutex so an old health sample cannot restart the newly attached session. App launch uses live session status instead of cached health.

The UI and daemon must be updated together for these new D-Bus methods. No Android image, user data, graphics driver or root configuration is changed by this fix.
