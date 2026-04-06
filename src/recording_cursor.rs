/// JavaScript to inject a visible cursor overlay into the page DOM.
/// The cursor is a fixed-position SVG element that stays on top of everything.
/// Uses a 0.6s cubic-bezier transition so movement is captured across multiple
/// frames at 2fps (each frame = 500ms).
pub const INJECT_CURSOR_JS: &str = r#"
(() => {
    if (document.getElementById('__pr_cursor')) return;
    const c = document.createElement('div');
    c.id = '__pr_cursor';
    c.innerHTML = '<svg width="28" height="28" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M5 3L19 12L12 13L9 20L5 3Z" fill="white" stroke="black" stroke-width="2" stroke-linejoin="round"/></svg>';
    c.style.cssText = 'position:fixed;top:0;left:0;z-index:2147483647;pointer-events:none;transition:transform 0.6s cubic-bezier(0.22,0.61,0.36,1);transform:translate(-100px,-100px);will-change:transform;';
    document.body.appendChild(c);
})()
"#;

/// JavaScript to move the cursor to (x, y) with smooth animation.
pub fn move_cursor_js(x: f64, y: f64) -> String {
    format!(
        r#"(() => {{
            const c = document.getElementById('__pr_cursor');
            if (c) c.style.transform = 'translate({}px,{}px)';
        }})()"#,
        x, y
    )
}

/// JavaScript to remove the cursor overlay.
pub const REMOVE_CURSOR_JS: &str = r#"
(() => {
    const c = document.getElementById('__pr_cursor');
    if (c) c.remove();
})()
"#;

/// JavaScript to add a click ripple animation at (x, y).
pub fn click_ripple_js(x: f64, y: f64) -> String {
    format!(
        r#"(() => {{
            const r = document.createElement('div');
            r.style.cssText = `position:fixed;z-index:2147483646;pointer-events:none;
                left:{}px;top:{}px;width:0;height:0;border-radius:50%;
                border:2px solid rgba(59,130,246,0.7);
                transform:translate(-50%,-50%) scale(0);
                animation:__pr_ripple 0.5s cubic-bezier(0,0,0.2,1) forwards;`;
            document.body.appendChild(r);
            if (!document.getElementById('__pr_ripple_style')) {{
                const s = document.createElement('style');
                s.id = '__pr_ripple_style';
                s.textContent = `@keyframes __pr_ripple {{
                    0%   {{ transform:translate(-50%,-50%) scale(0); opacity:1; border-width:3px; }}
                    100% {{ transform:translate(-50%,-50%) scale(1); opacity:0; border-width:1px; width:50px;height:50px; }}
                }}`;
                document.head.appendChild(s);
            }}
            setTimeout(() => r.remove(), 600);
        }})()"#,
        x, y
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_cursor_js_is_valid() {
        assert!(INJECT_CURSOR_JS.contains("__pr_cursor"));
        assert!(INJECT_CURSOR_JS.contains("svg"));
        // Should have a slow transition for smooth capture
        assert!(INJECT_CURSOR_JS.contains("0.6s"));
    }

    #[test]
    fn test_move_cursor_js() {
        let js = move_cursor_js(100.0, 200.0);
        assert!(js.contains("100px"));
        assert!(js.contains("200px"));
        assert!(js.contains("__pr_cursor"));
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
