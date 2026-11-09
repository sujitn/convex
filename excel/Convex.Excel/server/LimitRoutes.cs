using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Net;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;

namespace Convex.Excel.Server
{
    /// A limit route's answer: the status read on success, else what failed.
    internal sealed class LimitAnswer
    {
        /// Null when the server could not be reached.
        public HttpStatusCode? Status { get; set; }
        public LimitBookStatus? Read { get; set; }
        public TimeSpan? RetryAfter { get; set; }
        /// The server's error code (`NOT_FOUND`, `BOOK_ROWS_REJECTED`, …).
        public string? Code { get; set; }
        /// What failed, for a person.
        public string Problem { get; set; } = "";
        /// For BOOK_ROWS_REJECTED: each refused row (0-based) and why.
        public List<(int Row, string Reason)> Rows { get; } = new();
    }

    /// The book limit routes the add-in reads (server P14b, P14d):
    /// `GET /api/v1/books/{id}/limits/status` and
    /// `POST /api/v1/books/{id}/limits/check`. Neither writes anything.
    internal static class LimitRoutes
    {
        public static Task<LimitAnswer> StatusAsync(HttpClient http, Uri baseUri, string? token, string bookId,
            CancellationToken ct) =>
            SendAsync(http, HttpMethod.Get, Route(baseUri, bookId, "status"), token, null, bookId, ct);

        /// What status would say with `changes` (signed quantities added to
        /// the held positions) applied; nothing is stored.
        public static Task<LimitAnswer> CheckAsync(HttpClient http, Uri baseUri, string? token, string bookId,
            IEnumerable<LimitChangeRow> changes, CancellationToken ct)
        {
            var body = new JObject { ["changes"] = new JArray(changes.Select(c => c.ToJson())) };
            return SendAsync(http, HttpMethod.Post, Route(baseUri, bookId, "check"), token, body, bookId, ct);
        }

        private static Uri Route(Uri baseUri, string bookId, string leaf) =>
            new(baseUri, "api/v1/books/" + Uri.EscapeDataString(bookId) + "/limits/" + leaf);

        private static async Task<LimitAnswer> SendAsync(HttpClient http, HttpMethod method, Uri uri, string? token,
            JObject? body, string bookId, CancellationToken ct)
        {
            var answer = new LimitAnswer();
            try
            {
                using var request = new HttpRequestMessage(method, uri);
                if (body != null)
                    request.Content = new StringContent(body.ToString(Formatting.None), Encoding.UTF8, "application/json");
                if (token != null) request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", token);
                using var response = await http.SendAsync(request, ct).ConfigureAwait(false);
                var text = await response.Content.ReadAsStringAsync().ConfigureAwait(false);
                answer.Status = response.StatusCode;
                answer.RetryAfter = response.Headers.RetryAfter?.Delta;
                JObject? json = null;
                try { json = text.Length > 0 ? ServerJson.Parse(text) : null; }
                catch (JsonException) { /* not JSON: the status says enough */ }
                if (response.StatusCode == HttpStatusCode.OK)
                {
                    bool demo = response.Headers.TryGetValues("x-convex-data-quality", out var quality) &&
                                quality.Contains("demo", StringComparer.OrdinalIgnoreCase);
                    answer.Read = json == null ? null : LimitBookStatus.Parse(json, demo);
                    if (answer.Read == null) answer.Problem = $"the server's limit status of book {bookId} could not be read";
                    return answer;
                }
                answer.Code = ServerJson.Text(json?["code"]);
                var error = ServerJson.Text(json?["error"]);
                foreach (var row in (json?["rows"] as JArray ?? new JArray()).OfType<JObject>())
                {
                    var index = ServerJson.Number(row["index"]);
                    if (index == null) continue;
                    answer.Rows.Add(((int)index.Value,
                        (ServerJson.Text(row["code"]) ?? "") + ": " + (ServerJson.Text(row["reason"]) ?? "")));
                }
                answer.Problem = Describe(response.StatusCode, answer.Code, error, bookId);
            }
            catch (Exception ex) when (ex is HttpRequestException or TaskCanceledException && !ct.IsCancellationRequested)
            {
                answer.Problem = "could not reach the server: " + Cause(ex);
            }
            return answer;
        }

        private static string Describe(HttpStatusCode status, string? code, string? error, string bookId)
        {
            var detail = error != null ? " (" + error + ")" : "";
            return (int)status switch
            {
                401 => "the server no longer accepts the sign-in",
                403 => $"you are not entitled to the limits of book {bookId}: they need read access to the book, " +
                       "each instrument it holds and each one a limit names (FORBIDDEN)",
                404 => $"the server has no limit set for book {bookId}" + detail,
                429 => "the server asked to wait (too many requests)",
                _ => $"the server refused it: HTTP {((int)status).ToString(CultureInfo.InvariantCulture)}" +
                     (code != null ? " " + code : "") + detail,
            };
        }

        private static string Cause(Exception ex)
        {
            while (ex.InnerException != null) ex = ex.InnerException;
            return ex.Message.TrimEnd(' ', '.');
        }
    }

    /// One row of a pre-trade check: a signed quantity added to the book's
    /// position in an instrument, named by the server's instrument id or by
    /// a CUSIP or ISIN, which the server resolves (server D89, D91).
    internal sealed class LimitChangeRow
    {
        public string Identifier { get; set; } = "";
        /// Signed, as exact decimal text.
        public string Quantity { get; set; } = "";
        public string? Rating { get; set; }
        public string? Sector { get; set; }

        public JObject ToJson()
        {
            var (kind, value) = Identifiers.Classify(Identifier);
            var row = new JObject();
            if (kind == IdentifierKind.Instrument) row["instrument_id"] = value;
            else row["identifier"] = new JObject { ["kind"] = kind == IdentifierKind.Cusip ? "cusip" : "isin", ["value"] = value };
            row["quantity"] = Quantity;
            if (!string.IsNullOrWhiteSpace(Rating)) row["rating"] = Rating!.Trim();
            if (!string.IsNullOrWhiteSpace(Sector)) row["sector"] = Sector!.Trim();
            return row;
        }

        /// A signed quantity: digits with an optional sign, decimal point and
        /// thousands separators, not zero. Returns the exact text sent.
        public static string ParseQuantity(object? cell)
        {
            string text = cell switch
            {
                double d => d.ToString("R", CultureInfo.InvariantCulture),
                null => "",
                _ => Convert.ToString(cell, CultureInfo.InvariantCulture)?.Trim().Replace(",", "") ?? "",
            };
            if (!decimal.TryParse(text, NumberStyles.AllowLeadingSign | NumberStyles.AllowDecimalPoint | NumberStyles.AllowExponent,
                    CultureInfo.InvariantCulture, out var value))
                throw new ConvexException($"'{cell}' is not a signed quantity (for example 1000000 or -250000)");
            if (value == 0) throw new ConvexException("a quantity of 0 changes nothing");
            return value.ToString(CultureInfo.InvariantCulture);
        }
    }
}
