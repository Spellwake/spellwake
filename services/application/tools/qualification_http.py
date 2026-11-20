"""Credential-safe HTTP boundaries shared by local qualification drivers."""
import json
import urllib.error
import urllib.request


class NoRedirect(urllib.request.HTTPRedirectHandler):
    """Never forward a fixture credential to a redirect destination."""

    def redirect_request(self, *_: object, **__: object) -> None:
        return None


def make_opener() -> urllib.request.OpenerDirector:
    """Fixture traffic never uses inherited HTTP proxies."""
    return urllib.request.build_opener(NoRedirect(), urllib.request.ProxyHandler({}))


def request_json_with_headers(opener: urllib.request.OpenerDirector, request: urllib.request.Request,
                 timeout: float = 10) -> tuple[int, object, dict[str, str]]:
    """Bound decoding and discard vendor diagnostics from raised failures."""
    try:
        response = opener.open(request, timeout=timeout)
    except urllib.error.HTTPError as error:
        response = error
    except (OSError, ValueError):
        raise OSError('Qualification HTTP transport failed') from None
    with response:
        try:
            data = response.read(1024 * 1024 + 1)
        except OSError:
            raise OSError('Qualification HTTP response failed') from None
        if len(data) > 1024 * 1024:
            raise RuntimeError('Qualification HTTP response exceeds its bound')
        try:
            return response.status, json.loads(data) if data else None, {key.lower(): value for key, value in response.headers.items()}
        except (ValueError, UnicodeError):
            raise ValueError('Qualification HTTP JSON is invalid') from None


def request_json(opener: urllib.request.OpenerDirector, request: urllib.request.Request,
                 timeout: float = 10) -> tuple[int, object]:
    """Keep the established two-value interface over the single bounded HTTP reader."""
    status, payload, _ = request_json_with_headers(opener, request, timeout)
    return status, payload


def object_json(value: object) -> dict:
    """Endpoints expecting objects cannot silently accept another JSON shape."""
    if not isinstance(value, dict):
        raise RuntimeError('Qualification HTTP object response required')
    return value
