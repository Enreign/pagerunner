# Pagerunner Examples

Real-world use cases and code examples for Pagerunner. All examples use the CLI directly; the same commands work through the MCP interface in Claude Code via `/mcp`.

## Table of Contents

1. [Web Scraping](#web-scraping)
2. [Form Automation](#form-automation)
3. [Content Extraction](#content-extraction)
4. [Multi-Step Workflows](#multi-step-workflows)
5. [Security Testing](#security-testing)
6. [Data Collection](#data-collection)
7. [Session Persistence](#session-persistence)

---

## Web Scraping

### Example 1: Scrape Product Prices

Fetch product prices from a website with JavaScript rendering:

```bash
#!/bin/bash
# Setup
SESSION_ID=$(pagerunner open-session default | jq -r '.session_id')
TABS=$(pagerunner list-tabs $SESSION_ID | jq -r '.[0].target_id')

# Navigate to product page
pagerunner navigate $SESSION_ID $TABS "https://example.com/products"
pagerunner wait-for $SESSION_ID $TABS --selector ".price-list"

# Extract prices using JavaScript
PRICES=$(pagerunner evaluate $SESSION_ID $TABS '
  Array.from(document.querySelectorAll(".product"))
    .map(el => ({
      name: el.querySelector(".product-name")?.textContent,
      price: parseFloat(el.querySelector(".price")?.textContent || 0),
      url: el.querySelector("a")?.href
    }))
')

echo "$PRICES" | jq '.'

# Cleanup
pagerunner close-session $SESSION_ID
```

**Key Techniques:**
- Return labeled objects (not arrays) to prevent hallucination
- Use `.map()` for consistent structure
- Wait for elements before extracting

### Example 2: Scrape Paginated Content

Handle multi-page scraping with navigation:

```bash
#!/bin/bash
SESSION_ID=$(pagerunner open-session default | jq -r '.session_id')
TARGET_ID=$(pagerunner list-tabs $SESSION_ID | jq -r '.[0].target_id')
pagerunner navigate $SESSION_ID $TARGET_ID "https://example.com/listings?page=1"

RESULTS=()
for PAGE in {1..10}; do
  # Wait for page load
  pagerunner wait-for $SESSION_ID $TARGET_ID --selector ".listing-item"
  
  # Extract current page
  ITEMS=$(pagerunner evaluate $SESSION_ID $TARGET_ID '
    Array.from(document.querySelectorAll(".listing-item"))
      .map(el => ({
        title: el.textContent.trim(),
        url: el.querySelector("a")?.href
      }))
  ')
  RESULTS+=("$ITEMS")
  
  # Go to next page
  NEXT_PAGE=$((PAGE + 1))
  pagerunner navigate $SESSION_ID $TARGET_ID "https://example.com/listings?page=$NEXT_PAGE"
  sleep 1  # Be respectful
done

echo "${RESULTS[@]}" | jq -s 'add'
pagerunner close-session $SESSION_ID
```

---

## Form Automation

### Example 3: Login and Check Account

Automated login with session persistence:

```bash
#!/bin/bash
# Setup
SESSION_ID=$(pagerunner open-session default | jq -r '.session_id')
TARGET_ID=$(pagerunner list-tabs $SESSION_ID | jq -r '.[0].target_id')

# Navigate to login page
pagerunner navigate $SESSION_ID $TARGET_ID "https://app.example.com/login"
pagerunner wait-for $SESSION_ID $TARGET_ID --selector "form"

# Fill login form
pagerunner fill $SESSION_ID $TARGET_ID "[type='email']" "user@example.com"
pagerunner fill $SESSION_ID $TARGET_ID "[type='password']" "password123"
pagerunner click $SESSION_ID $TARGET_ID "button[type='submit']"

# Wait for dashboard
pagerunner wait-for $SESSION_ID $TARGET_ID --url "**/dashboard"

# Extract account info
ACCOUNT=$(pagerunner evaluate $SESSION_ID $TARGET_ID '
  ({
    username: document.querySelector(".username")?.textContent,
    balance: document.querySelector(".balance")?.textContent,
    status: document.querySelector(".status")?.textContent
  })
')

echo "Account Info:" "$ACCOUNT" | jq '.'

# Save session for later use
pagerunner save-snapshot $SESSION_ID $TARGET_ID --origin "https://app.example.com"
pagerunner save-tab-state $SESSION_ID

pagerunner close-session $SESSION_ID
```

### Example 4: Fill Complex Forms with Custom Fields

Handle forms with special input types (React, Vue, etc.):

```bash
#!/bin/bash
SESSION_ID=$(pagerunner open-session default | jq -r '.session_id')
TARGET_ID=$(pagerunner list-tabs $SESSION_ID | jq -r '.[0].target_id')

pagerunner navigate $SESSION_ID $TARGET_ID "https://example.com/signup"

# Basic text inputs
pagerunner fill $SESSION_ID $TARGET_ID "#firstName" "John"
pagerunner fill $SESSION_ID $TARGET_ID "#lastName" "Doe"
pagerunner fill $SESSION_ID $TARGET_ID "#email" "john@example.com"

# Dropdown select
pagerunner select $SESSION_ID $TARGET_ID "#country" "US"

# Radio button (click the input)
pagerunner click $SESSION_ID $TARGET_ID "#plan-pro"

# Checkbox
pagerunner click $SESSION_ID $TARGET_ID "#agree-terms"

# Rich text editor / React input (use fill which triggers synthetic events)
pagerunner fill $SESSION_ID $TARGET_ID "[data-testid='message']" "My message"

# Submit
pagerunner click $SESSION_ID $TARGET_ID "button[type='submit']"

# Wait for success
pagerunner wait-for $SESSION_ID $TARGET_ID --selector ".success-message"

pagerunner close-session $SESSION_ID
```

---

## Content Extraction

### Example 5: Extract Structured Data from Dynamic Page

Use evaluate to extract and structure nested data:

```bash
#!/bin/bash
SESSION_ID=$(pagerunser open-session default | jq -r '.session_id')
TARGET_ID=$(pagerunner list-tabs $SESSION_ID | jq -r '.[0].target_id')

pagerunner navigate $SESSION_ID $TARGET_ID "https://example.com/articles"
pagerunner wait-for $SESSION_ID $TARGET_ID --selector ".article"

# Extract with proper structure
ARTICLES=$(pagerunner evaluate $SESSION_ID $TARGET_ID '
  Array.from(document.querySelectorAll(".article"))
    .map(article => ({
      id: article.getAttribute("data-id"),
      title: article.querySelector("h2")?.textContent.trim(),
      author: article.querySelector(".author")?.textContent.trim(),
      published_date: article.querySelector("[data-date]")?.getAttribute("data-date"),
      excerpt: article.querySelector(".excerpt")?.textContent.trim(),
      tags: Array.from(article.querySelectorAll(".tag")).map(t => t.textContent.trim()),
      link: article.querySelector("a")?.href,
      likes: parseInt(article.querySelector(".like-count")?.textContent || 0),
      comments: parseInt(article.querySelector(".comment-count")?.textContent || 0)
    }))
')

echo "$ARTICLES" | jq '.'

pagerunner close-session $SESSION_ID
```

### Example 6: Get Page Content with Anonymization

Extract content with PII automatically anonymized:

```bash
#!/bin/bash
# Enable anonymization to strip PII before content reaches LLM
SESSION_ID=$(pagerunner open-session default --anonymize | jq -r '.session_id')
TARGET_ID=$(pagerunner list-tabs $SESSION_ID | jq -r '.[0].target_id')

pagerunner navigate $SESSION_ID $TARGET_ID "https://example.com/user-profile"
pagerunner wait-for $SESSION_ID $TARGET_ID --selector ".profile"

# This content will have PII stripped (EMAIL → [EMAIL], PHONE → [PHONE], etc.)
CONTENT=$(pagerunner get-content $SESSION_ID $TARGET_ID)

echo "Content (anonymized):"
echo "$CONTENT"

pagerunner close-session $SESSION_ID
```

---

## Multi-Step Workflows

### Example 7: Complete E-Commerce Purchase Workflow

Full checkout process with error handling:

```bash
#!/bin/bash
set -e

SESSION_ID=$(pagerunner open-session default | jq -r '.session_id')
TARGET_ID=$(pagerunner list-tabs $SESSION_ID | jq -r '.[0].target_id')

echo "Starting checkout workflow..."

# Navigate to store
pagerunner navigate $SESSION_ID $TARGET_ID "https://shop.example.com"
pagerunner wait-for $SESSION_ID $TARGET_ID --selector ".product"

# Add item to cart
pagerunner click $SESSION_ID $TARGET_ID "[data-product-id='12345'] .add-to-cart"
pagerunner wait-for $SESSION_ID $TARGET_ID --ms 1000

# Go to cart
pagerunner click $SESSION_ID $TARGET_ID ".cart-link"
pagerunner wait-for $SESSION_ID $TARGET_ID --selector ".checkout-button"

# Proceed to checkout
pagerunner click $SESSION_ID $TARGET_ID ".checkout-button"
pagerunner wait-for $SESSION_ID $TARGET_ID --selector "form.checkout"

# Fill shipping address
pagerunner fill $SESSION_ID $TARGET_ID "#address" "123 Main St"
pagerunner fill $SESSION_ID $TARGET_ID "#city" "Springfield"
pagerunner select $SESSION_ID $TARGET_ID "#state" "IL"
pagerunner fill $SESSION_ID $TARGET_ID "#zip" "62701"

# Continue to payment
pagerunner click $SESSION_ID $TARGET_ID ".next-step"
pagerunner wait-for $SESSION_ID $TARGET_ID --selector ".payment-form"

# Fill payment info (note: never use real credit cards)
# In production, use anonymization and tokens
pagerunner fill $SESSION_ID $TARGET_ID "#card-number" "4111111111111111"
pagerunner fill $SESSION_ID $TARGET_ID "#expiry" "12/25"
pagerunner fill $SESSION_ID $TARGET_ID "#cvc" "123"

# Agree and submit
pagerunner click $SESSION_ID $TARGET_ID "#agree-terms"
pagerunner click $SESSION_ID $TARGET_ID ".place-order"

# Confirm order
pagerunner wait-for $SESSION_ID $TARGET_ID --url "**/order-confirmation/**"
ORDER_NUM=$(pagerunner evaluate $SESSION_ID $TARGET_ID 'document.querySelector(".order-number")?.textContent')

echo "Order placed: $ORDER_NUM"

# Save confirmation
pagerunner screenshot $SESSION_ID $TARGET_ID --base64 > order_confirmation.json

pagerunner close-session $SESSION_ID
```

### Example 8: Data Migration Between Systems

Fetch data from source, transform, and push to destination:

```bash
#!/bin/bash
SESSION_ID=$(pagerunner open-session default | jq -r '.session_id')
TARGET_ID=$(pagerunner list-tabs $SESSION_ID | jq -r '.[0].target_id')

# Source: Old system
pagerunner navigate $SESSION_ID $TARGET_ID "https://old-system.example.com/export"
pagerunner wait-for $SESSION_ID $TARGET_ID --selector ".data-table"

# Extract data
SOURCE_DATA=$(pagerunner evaluate $SESSION_ID $TARGET_ID '
  Array.from(document.querySelectorAll("tr"))
    .slice(1) // skip header
    .map(row => ({
      id: row.cells[0].textContent.trim(),
      name: row.cells[1].textContent.trim(),
      value: row.cells[2].textContent.trim()
    }))
')

echo "Extracted $($SOURCE_DATA | jq 'length') records"

# Destination: New system
pagerunner navigate $SESSION_ID $TARGET_ID "https://new-system.example.com/import"
pagerunner wait-for $SESSION_ID $TARGET_ID --selector "form"

# Fill import form
pagerunner fill $SESSION_ID $TARGET_ID "#json-input" "$SOURCE_DATA"
pagerunner click $SESSION_ID $TARGET_ID ".import-button"

# Verify import
pagerunner wait-for $SESSION_ID $TARGET_ID --selector ".success-message"
RESULT=$(pagerunner get-content $SESSION_ID $TARGET_ID)

if [[ $RESULT == *"imported"* ]]; then
  echo "✓ Import successful"
else
  echo "✗ Import failed"
fi

pagerunner close-session $SESSION_ID
```

---

## Security Testing

### Example 9: Test Your Site's Anti-Bot Measures

Verify your security controls are working:

```bash
#!/bin/bash
# Use stealth mode to test anti-bot detection
SESSION_ID=$(pagerunner open-session default --stealth | jq -r '.session_id')
TARGET_ID=$(pagerunner list-tabs $SESSION_ID | jq -r '.[0].target_id')

# Navigate normally
pagerunner navigate $SESSION_ID $TARGET_ID "https://mysite.example.com"
pagerunner wait-for $SESSION_ID $TARGET_ID --ms 2000

# Check for anti-bot challenges
BLOCKED=$(pagerunner evaluate $SESSION_ID $TARGET_ID '
  ({
    has_captcha: !!document.querySelector("[data-captcha]"),
    has_challenge: !!document.querySelector(".challenge-modal"),
    detected_webdriver: window.navigator.webdriver,
    detected_automation: !!window.__automation__,
    custom_checks: document.querySelector("[data-automation-detected]")?.textContent
  })
')

echo "Anti-bot Test Results:"
echo "$BLOCKED" | jq '.'

pagerunner close-session $SESSION_ID
```

### Example 10: Validate HTTPS and Security Headers

Check SSL and security configurations:

```bash
#!/bin/bash
SESSION_ID=$(pagerunner open-session default | jq -r '.session_id')
TARGET_ID=$(pagerunner list-tabs $SESSION_ID | jq -r '.[0].target_id')

pagerunner navigate $SESSION_ID $TARGET_ID "https://example.com"

# Check security headers via JavaScript (Network tab unavailable via CDP)
SECURITY=$(pagerunner evaluate $SESSION_ID $TARGET_ID '
  ({
    url: window.location.href,
    protocol: window.location.protocol,
    is_https: window.location.protocol === "https:",
    cert_pinning_header: document.querySelector("link[rel=pin-cert]")?.href,
    // Note: Most security headers are sent in response headers
    // Access via Service Worker or network monitoring (not shown here)
  })
')

echo "$SECURITY" | jq '.'

pagerunner close-session $SESSION_ID
```

---

## Data Collection

### Example 11: Market Research Data Collection

Gather competitive intelligence with proper attribution:

```bash
#!/bin/bash
SESSION_ID=$(pagerunner open-session default | jq -r '.session_id')
TARGET_ID=$(pagerunner list-tabs $SESSION_ID | jq -r '.[0].target_id')

COMPETITORS=("competitor-a.com" "competitor-b.com" "competitor-c.com")
RESULTS=()

for COMPETITOR in "${COMPETITORS[@]}"; do
  echo "Analyzing $COMPETITOR..."
  
  pagerunner navigate $SESSION_ID $TARGET_ID "https://$COMPETITOR/pricing"
  pagerunner wait-for $SESSION_ID $TARGET_ID --selector ".pricing-tier"
  
  PRICING=$(pagerunner evaluate $SESSION_ID $TARGET_ID '
    Array.from(document.querySelectorAll(".pricing-tier"))
      .map(tier => ({
        name: tier.querySelector(".tier-name")?.textContent.trim(),
        price: tier.querySelector(".price")?.textContent.trim(),
        features: Array.from(tier.querySelectorAll(".feature"))
          .map(f => f.textContent.trim())
      }))
  ')
  
  RESULTS+=("{\"competitor\": \"$COMPETITOR\", \"data\": $PRICING}")
done

echo "Market Research Results:"
echo "${RESULTS[@]}" | jq -s 'add'

pagerunner close-session $SESSION_ID
```

---

## Session Persistence

### Example 12: Reuse Sessions Across Multiple Operations

Keep a session open for multiple related operations:

```bash
#!/bin/bash

# Open session once
SESSION_ID=$(pagerunner open-session "work-profile" | jq -r '.session_id')

# Operation 1: Check email
TARGET_ID=$(pagerunner list-tabs $SESSION_ID | jq -r '.[0].target_id')
pagerunner navigate $SESSION_ID $TARGET_ID "https://mail.example.com"
pagerunner wait-for $SESSION_ID $TARGET_ID --selector ".inbox"
EMAIL_COUNT=$(pagerunner evaluate $SESSION_ID $TARGET_ID 'document.querySelectorAll(".email").length')
echo "Inbox: $EMAIL_COUNT emails"

# Operation 2: Check calendar
pagerunner navigate $SESSION_ID $TARGET_ID "https://calendar.example.com"
pagerunner wait-for $SESSION_ID $TARGET_ID --selector ".calendar"
TODAY_EVENTS=$(pagerunner evaluate $SESSION_ID $TARGET_ID '
  Array.from(document.querySelectorAll("[data-date*=today]"))
    .map(e => ({title: e.textContent, time: e.getAttribute("data-time")}))
')
echo "Today events:" "$TODAY_EVENTS" | jq '.'

# Operation 3: Update profile
pagerunner navigate $SESSION_ID $TARGET_ID "https://settings.example.com/profile"
pagerunner fill $SESSION_ID $TARGET_ID "#status" "Available"
pagerunner click $SESSION_ID $TARGET_ID ".save-button"

# Keep the session open for more work
echo "Session $SESSION_ID is ready for more operations"
echo "Use: pagerunner close-session $SESSION_ID"
```

### Example 13: Save and Restore Session State

Persist credentials and state for reuse:

```bash
#!/bin/bash

# First time: login and save state
echo "=== First Run: Login and Save ==="
SESSION_ID=$(pagerunner open-session personal | jq -r '.session_id')
TARGET_ID=$(pagerunner list-tabs $SESSION_ID | jq -r '.[0].target_id')

pagerunner navigate $SESSION_ID $TARGET_ID "https://app.example.com"
pagerunner fill $SESSION_ID $TARGET_ID "[type='email']" "user@example.com"
pagerunner fill $SESSION_ID $TARGET_ID "[type='password']" "password"
pagerunner click $SESSION_ID $TARGET_ID "button[type='submit']"
pagerunner wait-for $SESSION_ID $TARGET_ID --url "**/dashboard"

# Save cookies and storage
pagerunner save-snapshot $SESSION_ID $TARGET_ID --origin "https://app.example.com"
pagerunner save-tab-state $SESSION_ID

echo "Session state saved"
pagerunner close-session $SESSION_ID

# Second time: restore state (no login needed)
echo "=== Second Run: Restore State ==="
SESSION_ID=$(pagerunner open-session personal | jq -r '.session_id')
TARGET_ID=$(pagerunner list-tabs $SESSION_ID | jq -r '.[0].target_id')

pagerunner navigate $SESSION_ID $TARGET_ID "https://app.example.com"
pagerunner restore-snapshot $SESSION_ID $TARGET_ID "https://app.example.com"
pagerunner restore-tab-state $SESSION_ID

# Already logged in!
CONTENT=$(pagerunner get-content $SESSION_ID $TARGET_ID)
echo "Logged in successfully: $CONTENT" | head -20

pagerunner close-session $SESSION_ID
```

---

## Tips & Best Practices

### Use Labeled Objects in `evaluate()`

❌ **Bad** (causes hallucination):
```javascript
// Returns [25, 2] → LLM guesses what these numbers mean
Array.from(divs).map(d => [d.views, d.likes])
```

✅ **Good** (prevents hallucination):
```javascript
// Returns {views: 25, likes: 2} → unambiguous
Array.from(divs).map(d => ({views: d.views, likes: d.likes}))
```

### Always Wait Before Extracting

```bash
pagerunner navigate $SESSION_ID $TARGET_ID "https://example.com"
pagerunner wait-for $SESSION_ID $TARGET_ID --selector ".loaded"  # Wait for rendering
CONTENT=$(pagerunner get-content $SESSION_ID $TARGET_ID)
```

### Handle Network Changes Gracefully

```bash
# Navigate with implicit wait for network idle
pagerunner navigate $SESSION_ID $TARGET_ID "https://example.com"

# If content depends on dynamic loading, wait explicitly
pagerunner wait-for $SESSION_ID $TARGET_ID --selector ".dynamic-content"

# Only then extract
DATA=$(pagerunner evaluate $SESSION_ID $TARGET_ID '...')
```

### Monitor Audit Logs

```bash
# Real-time monitoring during automation
tail -f ~/.pagerunner/audit.log | jq '.[] | {tool: .tool, error: .error}'

# Replay a session
pagerunner audit --session $SESSION_ID
```

### Use Anonymization for Sensitive Data

```bash
# Enable when working with PII
SESSION_ID=$(pagerunner open-session my-profile --anonymize | jq -r '.session_id')

# Content will be auto-anonymized: john@example.com → [EMAIL:a3f9b2]
```

---

**Ready to automate your workflows?** Start with a simple test, then build up to complex multi-step scenarios. Join us on [GitHub](https://github.com/Enreign/pagerunner) for more examples and to share your use cases!
