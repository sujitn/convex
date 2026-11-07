using System;
using System.Drawing;
using System.Windows.Forms;
using Convex.Excel.Server;
using static Convex.Excel.Helpers.FormUi;

namespace Convex.Excel.Forms
{
    // The server link settings (%APPDATA%\Convex\server.json): the server URL
    // and how to sign in. A pasted token is held in memory only.
    internal sealed class ServerSettingsForm : Form
    {
        private readonly TextBox _url = new();
        private readonly ComboBox _mode = new() { DropDownStyle = ComboBoxStyle.DropDownList };
        private readonly TextBox _issuer = new();
        private readonly TextBox _clientId = new();
        private readonly TextBox _scope = new();
        private readonly TextBox _resource = new();
        private readonly TextBox _redirectHost = new();
        private readonly TextBox _token = new() { UseSystemPasswordChar = true };
        private readonly Label _status = new() { AutoSize = true, ForeColor = Color.Gray };

        private static readonly string[] Modes =
        {
            "Single sign-on (OIDC)", "Pasted token (development)", "None (server in local development mode)",
        };

        public ServerSettingsForm()
        {
            Text = "Convex — Server";
            Size = new Size(620, 430);
            StartPosition = FormStartPosition.CenterParent;
            FormBorderStyle = FormBorderStyle.FixedDialog;
            MaximizeBox = MinimizeBox = false;

            _mode.Items.AddRange(Modes);
            var c = ServerLink.Config;
            _url.Text = c.Url;
            _mode.SelectedIndex = (int)c.SignIn;
            _issuer.Text = c.Issuer;
            _clientId.Text = c.ClientId;
            _scope.Text = c.Scope;
            _resource.Text = c.Resource ?? "";
            _redirectHost.Text = c.RedirectHost;
            _mode.SelectedIndexChanged += (_, _) => ShowMode();

            var tips = new ToolTip();
            tips.SetToolTip(_url, "https://host[:port] of convex-server (http:// only for this machine).");
            tips.SetToolTip(_resource, "Optional: sent as the OAuth `resource` parameter when your identity provider needs one.");
            tips.SetToolTip(_redirectHost, "Host of the loopback sign-in redirect: 127.0.0.1 (default) or localhost.");
            tips.SetToolTip(_token, "Held in memory only; never saved.");

            var grid = new TableLayoutPanel
            {
                Dock = DockStyle.Top, Height = 340, ColumnCount = 2, RowCount = 8, Padding = new Padding(12),
            };
            grid.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 150));
            grid.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            for (int i = 0; i < 8; i++) grid.RowStyles.Add(new RowStyle(SizeType.Absolute, 38));
            AddRow(grid, 0, "Server URL:", _url);
            AddRow(grid, 1, "Sign in with:", _mode);
            AddRow(grid, 2, "Issuer:", _issuer);
            AddRow(grid, 3, "Client id:", _clientId);
            AddRow(grid, 4, "Scope:", _scope);
            AddRow(grid, 5, "Resource:", _resource);
            AddRow(grid, 6, "Redirect host:", _redirectHost);
            AddRow(grid, 7, "Token:", _token);

            var bottom = new FlowLayoutPanel { Dock = DockStyle.Bottom, Height = 44, Padding = new Padding(12, 6, 12, 6) };
            bottom.Controls.AddRange(new Control[]
            {
                NewButton("Save and connect", (_, _) => Save()),
                NewButton("Close", (_, _) => Close()),
                _status,
            });
            Controls.Add(grid);
            Controls.Add(bottom);
            ShowMode();
        }

        private void ShowMode()
        {
            bool oidc = _mode.SelectedIndex == (int)SignInMode.Oidc;
            _issuer.Enabled = _clientId.Enabled = _scope.Enabled = _resource.Enabled = _redirectHost.Enabled = oidc;
            _token.Enabled = _mode.SelectedIndex == (int)SignInMode.Token;
        }

        private void Save()
        {
            var config = new ServerConfig
            {
                Url = _url.Text.Trim(),
                SignIn = (SignInMode)_mode.SelectedIndex,
                Issuer = _issuer.Text.Trim(),
                ClientId = _clientId.Text.Trim(),
                Scope = _scope.Text.Trim(),
                Resource = _resource.Text.Trim().Length > 0 ? _resource.Text.Trim() : null,
                RedirectHost = _redirectHost.Text.Trim().Length > 0 ? _redirectHost.Text.Trim() : "127.0.0.1",
            };
            try
            {
                if (config.Configured) config.HttpBase();
                if (config.Configured && config.SignIn == SignInMode.Oidc &&
                    (config.Issuer.Length == 0 || config.ClientId.Length == 0))
                    throw new ConvexException("single sign-on needs the issuer and the client id");
                config.Save();
                ServerLink.Apply(config, config.SignIn == SignInMode.Token ? _token.Text.Trim() : null);
                _status.ForeColor = Color.Gray;
                _status.Text = config.SignIn == SignInMode.Oidc ? "Saved. Use Sign in on the ribbon." : "Saved; connecting.";
            }
            catch (Exception ex)
            {
                _status.ForeColor = Color.Firebrick;
                _status.Text = ex.Message;
            }
        }

        private static void AddRow(TableLayoutPanel grid, int row, string label, Control control)
        {
            grid.Controls.Add(
                new Label { Text = label, Anchor = AnchorStyles.Left, AutoSize = true, Padding = new Padding(0, 6, 0, 0) }, 0, row);
            control.Anchor = AnchorStyles.Left | AnchorStyles.Right;
            grid.Controls.Add(control, 1, row);
        }
    }
}
