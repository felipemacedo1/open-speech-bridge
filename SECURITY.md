# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in OpenSpeechBridge, please report it responsibly:

1. **Do NOT** open a public issue
2. Email the maintainers directly (or use GitHub's private vulnerability reporting)
3. Include:
   - Description of the vulnerability
   - Steps to reproduce
   - Potential impact
   - Suggested fix (if any)

We will respond within 48 hours and work with you to address the issue.

## Security Considerations

### Audio Privacy

- Audio is processed locally by default
- No audio is transmitted to external servers unless explicitly configured
- No conversation recording by default

### Model Security

- Only use models from trusted sources
- Verify model checksums when available
- Review model licenses before use

### Process Isolation

- ML engines run in separate processes
- Engine crashes should not compromise the main daemon
- Engines have limited system access

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.x.x   | :white_check_mark: |

Pre-1.0 versions receive security updates as needed.
