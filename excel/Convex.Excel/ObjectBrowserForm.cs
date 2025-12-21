using System;
using System.Collections.Generic;
using System.Drawing;
using System.Runtime.InteropServices;
using System.Windows.Forms;

namespace Convex.Excel
{
    /// <summary>
    /// Form to browse all registered objects in the Convex registry.
    /// </summary>
    public class ObjectBrowserForm : Form
    {
        private DataGridView dataGrid;
        private Button btnRefresh;
        private Button btnClose;
        private Button btnRelease;
        private ComboBox cboFilter;
        private Label lblCount;
        private List<ObjectInfo> objects;

        public ObjectBrowserForm()
        {
            InitializeComponent();
            RefreshObjects();
        }

        private void InitializeComponent()
        {
            this.Text = "Convex Object Browser";
            this.Size = new Size(700, 450);
            this.StartPosition = FormStartPosition.CenterScreen;
            this.MinimumSize = new Size(500, 300);
            this.FormBorderStyle = FormBorderStyle.Sizable;

            // Filter panel
            var filterPanel = new Panel
            {
                Dock = DockStyle.Top,
                Height = 40,
                Padding = new Padding(5)
            };

            var lblFilter = new Label
            {
                Text = "Filter:",
                Location = new Point(10, 12),
                AutoSize = true
            };

            cboFilter = new ComboBox
            {
                Location = new Point(50, 8),
                Width = 150,
                DropDownStyle = ComboBoxStyle.DropDownList
            };
            cboFilter.Items.Add("All Objects");
            cboFilter.Items.Add("Curves");
            cboFilter.Items.Add("Bonds");
            cboFilter.SelectedIndex = 0;
            cboFilter.SelectedIndexChanged += (s, e) => RefreshObjects();

            lblCount = new Label
            {
                Location = new Point(220, 12),
                AutoSize = true,
                Text = "0 objects"
            };

            filterPanel.Controls.Add(lblFilter);
            filterPanel.Controls.Add(cboFilter);
            filterPanel.Controls.Add(lblCount);

            // Data grid
            dataGrid = new DataGridView
            {
                Dock = DockStyle.Fill,
                AllowUserToAddRows = false,
                AllowUserToDeleteRows = false,
                ReadOnly = true,
                SelectionMode = DataGridViewSelectionMode.FullRowSelect,
                MultiSelect = false,
                AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.Fill,
                RowHeadersVisible = false,
                BackgroundColor = SystemColors.Window,
                BorderStyle = BorderStyle.Fixed3D
            };

            dataGrid.Columns.Add("Handle", "Handle");
            dataGrid.Columns.Add("Type", "Type");
            dataGrid.Columns.Add("Name", "Name");

            dataGrid.Columns["Handle"].Width = 80;
            dataGrid.Columns["Handle"].MinimumWidth = 60;
            dataGrid.Columns["Type"].Width = 120;
            dataGrid.Columns["Type"].MinimumWidth = 80;
            dataGrid.Columns["Name"].AutoSizeMode = DataGridViewAutoSizeColumnMode.Fill;

            // Button panel
            var buttonPanel = new Panel
            {
                Dock = DockStyle.Bottom,
                Height = 45,
                Padding = new Padding(5)
            };

            btnRefresh = new Button
            {
                Text = "Refresh",
                Width = 80,
                Location = new Point(10, 8)
            };
            btnRefresh.Click += (s, e) => RefreshObjects();

            btnRelease = new Button
            {
                Text = "Release",
                Width = 80,
                Location = new Point(100, 8)
            };
            btnRelease.Click += BtnRelease_Click;

            btnClose = new Button
            {
                Text = "Close",
                Width = 80,
                Anchor = AnchorStyles.Right | AnchorStyles.Bottom
            };
            btnClose.Location = new Point(buttonPanel.Width - btnClose.Width - 15, 8);
            btnClose.Click += (s, e) => this.Close();

            buttonPanel.Controls.Add(btnRefresh);
            buttonPanel.Controls.Add(btnRelease);
            buttonPanel.Controls.Add(btnClose);

            // Add controls
            this.Controls.Add(dataGrid);
            this.Controls.Add(filterPanel);
            this.Controls.Add(buttonPanel);

            // Handle resize for button positioning
            this.Resize += (s, e) =>
            {
                btnClose.Location = new Point(buttonPanel.Width - btnClose.Width - 15, 8);
            };
        }

        private void RefreshObjects()
        {
            objects = new List<ObjectInfo>();
            dataGrid.Rows.Clear();

            int filterType = 0;
            if (cboFilter.SelectedIndex == 1) filterType = 1; // Curves
            else if (cboFilter.SelectedIndex == 2) filterType = 2; // Bonds (FixedBond)

            // Use callback to enumerate objects
            NativeMethods.ObjectEnumCallback callback = (handle, objType, namePtr) =>
            {
                string name = namePtr != IntPtr.Zero ? Marshal.PtrToStringAnsi(namePtr) : "";

                // For bonds filter, include all bond types (2-5)
                if (filterType == 2 && (objType < 2 || objType > 5))
                    return;

                objects.Add(new ObjectInfo
                {
                    Handle = handle,
                    TypeCode = objType,
                    Name = name
                });
            };

            // Call native enumerate - use filterType 0 for curves filter too since we handle it
            NativeMethods.convex_enumerate_objects(callback, filterType == 1 ? 1 : 0);

            // Populate grid
            foreach (var obj in objects)
            {
                dataGrid.Rows.Add(
                    HandleHelper.Format(obj.Handle),
                    GetTypeName(obj.TypeCode),
                    string.IsNullOrEmpty(obj.Name) ? "(unnamed)" : obj.Name
                );
            }

            lblCount.Text = objects.Count + " object" + (objects.Count == 1 ? "" : "s");

            // Keep callback alive during enumeration
            GC.KeepAlive(callback);
        }

        private void BtnRelease_Click(object sender, EventArgs e)
        {
            if (dataGrid.SelectedRows.Count == 0)
            {
                MessageBox.Show("Please select an object to release.", "No Selection",
                    MessageBoxButtons.OK, MessageBoxIcon.Information);
                return;
            }

            int index = dataGrid.SelectedRows[0].Index;
            if (index >= 0 && index < objects.Count)
            {
                var obj = objects[index];
                var result = MessageBox.Show(
                    "Release object " + HandleHelper.Format(obj.Handle) + "?\n\n" +
                    "Type: " + GetTypeName(obj.TypeCode) + "\n" +
                    "Name: " + (string.IsNullOrEmpty(obj.Name) ? "(unnamed)" : obj.Name),
                    "Confirm Release",
                    MessageBoxButtons.YesNo,
                    MessageBoxIcon.Question);

                if (result == DialogResult.Yes)
                {
                    ConvexWrapper.Release(obj.Handle);
                    RefreshObjects();
                }
            }
        }

        private string GetTypeName(int typeCode)
        {
            switch (typeCode)
            {
                case 0: return "Unknown";
                case 1: return "Curve";
                case 2: return "Fixed Bond";
                case 3: return "Zero Bond";
                case 4: return "FRN";
                case 5: return "Callable Bond";
                case 6: return "Cash Flows";
                case 7: return "Price Result";
                case 8: return "Risk Result";
                case 9: return "Spread Result";
                case 10: return "YAS Result";
                default: return "Type " + typeCode;
            }
        }

        private class ObjectInfo
        {
            public ulong Handle { get; set; }
            public int TypeCode { get; set; }
            public string Name { get; set; }
        }
    }
}
