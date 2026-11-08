using System;
using System.Diagnostics;
using System.Net.Http;
using System.Threading;
using System.Threading.Tasks;

namespace Convex.Excel.Server
{
    /// The add-in's one server link: configuration, sign-in, the source, the
    /// identifier resolver and the hub's server cells.
    internal static class ServerLink
    {
        private static readonly object _lock = new();
        private static readonly HttpClient Http = new() { Timeout = TimeSpan.FromSeconds(30) };
        private static ServerConfig _config = new();
        private static OidcTokenProvider? _oidc;
        private static string? _pastedToken;

        public static ServerSource Source { get; } = new(new ClientWebSocketTransport());
        public static IdentifierResolver Resolver { get; } = new(Source, Http);
        public static ServerCells Cells { get; } = new(Source, Resolver);

        /// The connection or sign-in changed (for the ribbon); not raised for
        /// each message. Raised on any thread.
        public static event Action? Changed;

        static ServerLink()
        {
            Source.StatusChanged += link =>
            {
                if (link) Changed?.Invoke();
            };
        }

        public static ServerConfig Config
        {
            get { lock (_lock) return _config; }
        }

        /// Load the saved configuration (at add-in load). Sign-in waits for
        /// the user: an OIDC link starts signed out.
        public static void Start() => Apply(ServerConfig.Load(), null);

        /// Use `config`, keeping a pasted token only when one is given.
        public static void Apply(ServerConfig config, string? pastedToken)
        {
            ITokenProvider tokens;
            lock (_lock)
            {
                _config = config;
                _oidc?.Dispose();
                _oidc = null;
                if (pastedToken != null) _pastedToken = pastedToken.Length == 0 ? null : pastedToken;
                switch (config.SignIn)
                {
                    case SignInMode.None:
                        tokens = new NoTokenProvider();
                        break;
                    case SignInMode.Token:
                        tokens = new PastedTokenProvider(_pastedToken);
                        break;
                    default:
                        _oidc = new OidcTokenProvider(new OidcClient(new OidcSettings
                        {
                            Issuer = config.Issuer,
                            ClientId = config.ClientId,
                            Scope = config.Scope,
                            Resource = config.Resource,
                            RedirectHost = config.RedirectHost,
                        }, Http, OpenBrowser));
                        tokens = _oidc;
                        break;
                }
            }
            tokens.Changed += () => Changed?.Invoke();
            Resolver.Clear();
            Source.Configure(config.Configured ? config : null, tokens);
            // After Configure, so the lookups go to this server with this sign-in.
            if (config.Configured) Cells.RetryFailedLookups();
            Changed?.Invoke();
        }

        /// Whether Sign in applies (an OIDC link that is not signed in).
        public static bool CanSignIn
        {
            get { lock (_lock) return _config.Configured && _oidc is { Ready: false }; }
        }

        public static bool CanSignOut
        {
            get { lock (_lock) return _oidc is { Ready: true } || (_config.SignIn == SignInMode.Token && _pastedToken != null); }
        }

        /// Interactive sign-in in the system browser. Throws OidcException
        /// with a message for the user.
        public static async Task SignInAsync(CancellationToken ct)
        {
            OidcTokenProvider? oidc;
            lock (_lock) oidc = _oidc;
            if (oidc == null) throw new OidcException("this server link does not use single sign-on; see Server settings");
            await oidc.SignInAsync(ct).ConfigureAwait(false);
            // A sign-in may be a different user with other entitlements: try
            // refused instruments and failed lookups again, and connect now.
            Source.Reconnect();
            Resolver.Clear();
            Cells.RetryFailedLookups();
        }

        public static void SignOut()
        {
            ServerConfig config;
            lock (_lock)
            {
                _oidc?.SignOut();
                if (_oidc != null) return;
                _pastedToken = null;
                config = _config;
            }
            Apply(config, "");
        }

        private static void OpenBrowser(Uri uri) =>
            Process.Start(new ProcessStartInfo(uri.AbsoluteUri) { UseShellExecute = true });
    }
}
