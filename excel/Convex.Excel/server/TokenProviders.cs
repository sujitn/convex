using System;
using System.Text;
using Newtonsoft.Json.Linq;

namespace Convex.Excel.Server
{
    /// Where the server link gets its bearer token. The server reads a
    /// WebSocket's token only at the upgrade and closes the session when that
    /// token expires, so a renewed token reaches the server through a new
    /// session (ServerSource opens it before closing the old one).
    internal interface ITokenProvider
    {
        /// The bearer token to send, or null when the server needs none
        /// (a server in local development mode).
        string? Token { get; }

        /// Whether a connection may be attempted (false while signed out).
        bool Ready { get; }

        /// Who is signed in, for display.
        string User { get; }

        /// Why the provider is not ready, for display; null when ready.
        string? Problem { get; }

        /// Raised, on any thread, when Token, Ready or Problem changes.
        event Action? Changed;

        /// The server refused the current token (HTTP 401 or UNAUTHENTICATED).
        void Rejected();
    }

    /// A server in local development mode: no token at all.
    internal sealed class NoTokenProvider : ITokenProvider
    {
        public string? Token => null;
        public bool Ready => true;
        public string User => "(local development)";
        public string? Problem => null;
        public event Action? Changed { add { } remove { } }
        public void Rejected() { }
    }

    /// A token pasted into Settings, for development. It is held in memory
    /// only and never renewed: once the server refuses it, the link stays
    /// signed out until a new token is pasted.
    internal sealed class PastedTokenProvider : ITokenProvider
    {
        private readonly object _lock = new();
        private string? _token;
        private string? _problem;

        public PastedTokenProvider(string? token)
        {
            _token = string.IsNullOrWhiteSpace(token) ? null : token!.Trim();
            _problem = _token == null ? "no token: paste one in Convex Server settings" : null;
            User = _token == null ? "(no token)" : Jwt.DisplayName(_token) ?? "(pasted token)";
        }

        public string? Token { get { lock (_lock) return _token; } }
        public bool Ready { get { lock (_lock) return _token != null; } }
        public string User { get; }
        public string? Problem { get { lock (_lock) return _problem; } }
        public event Action? Changed;

        public void Rejected()
        {
            lock (_lock)
            {
                if (_token == null) return;
                _token = null;
                _problem = "the server refused the pasted token: paste a new one in Convex Server settings";
            }
            Changed?.Invoke();
        }
    }

    /// Reads display claims from a JWT without validating it. The server
    /// validates tokens (by introspection); the add-in only shows who is
    /// signed in. An opaque token gives null.
    internal static class Jwt
    {
        public static JObject? Payload(string token)
        {
            var parts = token.Split('.');
            if (parts.Length != 3) return null;
            try
            {
                var s = parts[1].Replace('-', '+').Replace('_', '/');
                s = s.PadRight(s.Length + (4 - s.Length % 4) % 4, '=');
                return JObject.Parse(Encoding.UTF8.GetString(Convert.FromBase64String(s)));
            }
            catch
            {
                return null;
            }
        }

        public static string? DisplayName(string token)
        {
            var payload = Payload(token);
            if (payload == null) return null;
            foreach (var claim in new[] { "preferred_username", "email", "name", "upn", "sub" })
                if (payload[claim] is JValue { Type: JTokenType.String } v && !string.IsNullOrEmpty((string?)v))
                    return (string)v!;
            return null;
        }
    }
}
