# Sentinel Journal

## 2025-05-18 - URL Validation Bypass with Protocol-Relative URLs and Inline Credentials
**Vulnerability:** `is_safe_url` in the web frontend allowed protocol-relative URLs starting with `//` (such as `//user:pass@domain/file`) to bypass inline credential (`@`) checks, and rejected uppercase HTTP/HTTPS scheme prefixes (`HTTP://`).
**Learning:** Checking for specific scheme prefixes (`http://`) without accounting for protocol-relative URLs (`//`) or case-insensitivity (`to_ascii_lowercase`) left fallback parsing logic vulnerable to credential leakage and false rejections.
**Prevention:** Normalize input to lowercase before scheme checks, explicitly handle protocol-relative `//` URLs, and parse host delimiters (`/`, `?`, `#`) consistently when inspecting host credentials.
