/// JavaScript to inject a visible cursor overlay into the page DOM.
/// Uses requestAnimationFrame for physics-based movement with momentum
/// and deceleration — much smoother than CSS transitions at any capture rate.
pub const INJECT_CURSOR_JS: &str = r#"
(() => {
    if (document.getElementById('__pr_cursor')) return;

    // Cursor container
    const c = document.createElement('div');
    c.id = '__pr_cursor';
    c.innerHTML = `<svg width="32" height="32" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
        <path d="M5 3L19 12L12 13L9 20L5 3Z" fill="white" stroke="black" stroke-width="1.8" stroke-linejoin="round"/>
    </svg>
    <div id="__pr_cursor_glow"></div>`;
    c.style.cssText = 'position:fixed;top:0;left:0;z-index:2147483647;pointer-events:none;will-change:transform;transform:translate(-100px,-100px);';

    // Glow effect behind cursor
    const glow = c.querySelector('#__pr_cursor_glow');
    glow.style.cssText = 'position:absolute;top:4px;left:4px;width:20px;height:20px;border-radius:50%;background:radial-gradient(circle,rgba(59,130,246,0.3) 0%,transparent 70%);filter:blur(8px);pointer-events:none;';

    document.body.appendChild(c);

    // Physics state
    window.__pr = window.__pr || {};
    window.__pr.cx = -100; window.__pr.cy = -100;  // current position
    window.__pr.tx = -100; window.__pr.ty = -100;  // target position
    window.__pr.vx = 0;    window.__pr.vy = 0;     // velocity
    window.__pr.animating = false;

    // Spring-based animation loop
    window.__pr.animate = function() {
        const p = window.__pr;
        const stiffness = 0.12;  // spring force
        const damping = 0.75;    // friction (1 = no friction)

        const dx = p.tx - p.cx;
        const dy = p.ty - p.cy;

        p.vx = (p.vx + dx * stiffness) * damping;
        p.vy = (p.vy + dy * stiffness) * damping;

        p.cx += p.vx;
        p.cy += p.vy;

        const el = document.getElementById('__pr_cursor');
        if (el) el.style.transform = 'translate(' + p.cx + 'px,' + p.cy + 'px)';

        // Stop when close enough and velocity is low
        if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5 && Math.abs(p.vx) < 0.3 && Math.abs(p.vy) < 0.3) {
            p.cx = p.tx; p.cy = p.ty;
            p.vx = 0; p.vy = 0;
            if (el) el.style.transform = 'translate(' + p.cx + 'px,' + p.cy + 'px)';
            p.animating = false;
            return;
        }
        requestAnimationFrame(p.animate);
    };
})()
"#;

/// JavaScript to set cursor target position — physics loop handles smooth movement.
pub fn move_cursor_js(x: f64, y: f64) -> String {
    format!(
        r#"(() => {{
            const p = window.__pr;
            if (!p) return;
            p.tx = {x}; p.ty = {y};
            if (!p.animating) {{
                p.animating = true;
                requestAnimationFrame(p.animate);
            }}
        }})()"#,
        x = x,
        y = y
    )
}

/// JavaScript to remove the cursor overlay.
#[allow(dead_code)]
pub const REMOVE_CURSOR_JS: &str = r#"
(() => {
    const c = document.getElementById('__pr_cursor');
    if (c) c.remove();
    if (window.__pr) { window.__pr.animating = false; }
})()
"#;

/// JavaScript to add a click ripple animation at (x, y).
pub fn click_ripple_js(x: f64, y: f64) -> String {
    format!(
        r#"(() => {{
            // Inject keyframes once
            if (!document.getElementById('__pr_ripple_style')) {{
                const s = document.createElement('style');
                s.id = '__pr_ripple_style';
                s.textContent = `
                    @keyframes __pr_ripple {{
                        0%   {{ transform:translate(-50%,-50%) scale(0); opacity:0.8; }}
                        100% {{ transform:translate(-50%,-50%) scale(1); opacity:0; }}
                    }}
                    @keyframes __pr_dot {{
                        0%   {{ transform:translate(-50%,-50%) scale(1); opacity:0.6; }}
                        100% {{ transform:translate(-50%,-50%) scale(0); opacity:0; }}
                    }}
                `;
                document.head.appendChild(s);
            }}

            // Outer ring
            const r = document.createElement('div');
            r.style.cssText = 'position:fixed;z-index:2147483646;pointer-events:none;left:{x}px;top:{y}px;width:40px;height:40px;border-radius:50%;border:2px solid rgba(59,130,246,0.6);animation:__pr_ripple 0.5s cubic-bezier(0,0,0.2,1) forwards;';
            document.body.appendChild(r);

            // Inner dot
            const d = document.createElement('div');
            d.style.cssText = 'position:fixed;z-index:2147483646;pointer-events:none;left:{x}px;top:{y}px;width:12px;height:12px;border-radius:50%;background:rgba(59,130,246,0.5);animation:__pr_dot 0.3s ease-out forwards;';
            document.body.appendChild(d);

            setTimeout(() => {{ r.remove(); d.remove(); }}, 600);
        }})()"#,
        x = x,
        y = y
    )
}

/// JavaScript to fade the page out (before navigation).
pub const FADE_OUT_JS: &str = r#"
(() => {
    const overlay = document.createElement('div');
    overlay.id = '__pr_fade';
    overlay.style.cssText = 'position:fixed;inset:0;z-index:2147483646;background:white;opacity:0;transition:opacity 0.3s ease-in;pointer-events:none;';
    document.body.appendChild(overlay);
    // Trigger reflow, then fade
    overlay.offsetHeight;
    overlay.style.opacity = '1';
})()
"#;

/// JavaScript to fade the page in (after navigation loads).
pub const FADE_IN_JS: &str = r#"
(() => {
    // Create a white overlay that fades out
    const overlay = document.createElement('div');
    overlay.id = '__pr_fade';
    overlay.style.cssText = 'position:fixed;inset:0;z-index:2147483646;background:white;opacity:1;transition:opacity 0.4s ease-out;pointer-events:none;';
    document.body.appendChild(overlay);
    overlay.offsetHeight;
    overlay.style.opacity = '0';
    setTimeout(() => overlay.remove(), 500);
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_cursor_js_has_physics() {
        assert!(INJECT_CURSOR_JS.contains("__pr_cursor"));
        assert!(INJECT_CURSOR_JS.contains("requestAnimationFrame"));
        assert!(INJECT_CURSOR_JS.contains("stiffness"));
    }

    #[test]
    fn test_move_cursor_js() {
        let js = move_cursor_js(100.0, 200.0);
        assert!(js.contains("100"));
        assert!(js.contains("200"));
        assert!(js.contains("__pr"));
    }

    #[test]
    fn test_remove_cursor_js() {
        assert!(REMOVE_CURSOR_JS.contains("remove()"));
    }

    #[test]
    fn test_click_ripple_js() {
        let js = click_ripple_js(150.0, 250.0);
        assert!(js.contains("150px"));
        assert!(js.contains("250px"));
        assert!(js.contains("__pr_ripple"));
    }
}
