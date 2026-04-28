using System;
using System.Drawing;
using System.Windows.Forms;
using Convex.Excel.Helpers;

namespace Convex.Excel.Forms
{
    // Renders convex_schema(...) output for any DTO. Useful for traders who
    // want to see exactly what fields a BondSpec or PricingRequest accepts,
    // and for tooling that wants to generate inputs programmatically.
    internal sealed class SchemaBrowserForm : Form
    {
        private static readonly string[] Types =
        {
            "Mark", "BondSpec", "CurveSpec",
            "PricingRequest", "PricingResponse",
            "RiskRequest", "RiskResponse",
            "SpreadRequest", "SpreadResponse",
            "CashflowRequest", "CashflowResponse",
            "CurveQueryRequest", "CurveQueryResponse",
        };

        private readonly ComboBox _type = new() { DropDownStyle = ComboBoxStyle.DropDownList, Width = 200 };
        private readonly TextBox _body = new()
        {
            Multiline = true, ReadOnly = true,
            ScrollBars = ScrollBars.Both, WordWrap = false,
            Font = new Font("Consolas", 9.5f),
            Dock = DockStyle.Fill,
            BackColor = Color.White,
        };
        private readonly Label _status = new() { AutoSize = true, ForeColor = Color.Gray };

        public SchemaBrowserForm()
        {
            Text = "Convex — Schema Browser";
            Size = new Size(720, 540);
            MinimumSize = new Size(560, 360);
            StartPosition = FormStartPosition.CenterParent;

            _type.Items.AddRange(Types);
            _type.SelectedIndex = 0;
            _type.SelectedIndexChanged += (_, _) => Render();

            var top = new FlowLayoutPanel
            {
                Dock = DockStyle.Top, Height = 36, Padding = new Padding(8, 6, 8, 6),
            };
            top.Controls.Add(new Label { Text = "Type:", AutoSize = true, Padding = new Padding(0, 5, 4, 0) });
            top.Controls.Add(_type);
            top.Controls.Add(NewButton("Copy to clipboard", (_, _) => Clipboard.SetText(_body.Text)));
            top.Controls.Add(NewButton("Stamp to sheet", (_, _) => StampToSheet()));

            var bottom = new FlowLayoutPanel
            {
                Dock = DockStyle.Bottom, Height = 36, Padding = new Padding(8, 6, 8, 6),
            };
            bottom.Controls.Add(_status);
            bottom.Controls.Add(NewButton("Close", (_, _) => Close()));

            Controls.Add(_body);
            Controls.Add(top);
            Controls.Add(bottom);

            Render();
        }

        private void Render()
        {
            try
            {
                var schema = Cx.Schema((string)_type.SelectedItem!);
                _body.Text = schema;
                _status.Text = "OK";
            }
            catch (Exception ex)
            {
                _body.Text = "";
                _status.Text = "ERROR: " + ex.Message;
            }
        }

        private void StampToSheet()
        {
            try
            {
                var addr = SheetHelpers.WriteFormulaAtSelection("=CX.SCHEMA(\"" + _type.SelectedItem + "\")");
                _status.Text = "Stamped =CX.SCHEMA at " + addr;
            }
            catch (Exception ex) { _status.Text = "ERROR: " + ex.Message; }
        }

        private static Button NewButton(string text, EventHandler onClick)
        {
            var b = new Button { Text = text, AutoSize = true, Padding = new Padding(8, 2, 8, 2) };
            b.Click += onClick;
            return b;
        }
    }
}
