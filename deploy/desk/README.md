# The desk demo

One command runs a trading desk on this machine: the server live on synthetic USD bonds,
the desk app, a market feed, and simulated clients and a booking system. Everything in it
is fictional and labelled demo; the server listens on loopback only, with no
authentication. It is not deployed anywhere.

From the repository root, with the repository's Rust toolchain, Node and Python 3.10+:

```sh
python3 deploy/desk/demo.py            # then open http://127.0.0.1:8080/desk/
```

Options: `--port`, `--work DIR` (default `target/desk-demo`, emptied at each launch),
`--debug` (a debug build, quicker to build and slower to run), `--no-build` (the binaries
and desk app already built) and `--rfq-rate` (client requests a second, default 0.2).
Ctrl-C stops everything, and so does any of its processes stopping. A run stops after 23
hours, since the seed's curves are fresh for a day: launch again for a new one. It refuses
a port already in use. Each process logs to the work directory's `logs/`.

## What it runs

1. **The seed** (`seed.py`), written for the launch's date, settling that day or the next
   weekday:
   - seven on-the-run Treasuries (2, 3, 5, 7, 10, 20 and 30 years) in the `UST-OTR`
     benchmark set, quoted by price;
   - ten investment grade bullets on a spread to the Treasury of their tenor, with
     widths for two-way quotes;
   - four high yield callables, two quoted by price and two by yield to worst;
   - two SOFR floating rate notes on their discount margin, with SOFR fixings through the
     last weekday;
   - two sinking-fund bonds by price per 100 current face, and two callable perpetuals by
     yield to worst;
   - three books, `BOOK-IG`, `BOOK-HY` and `BOOK-FLOW`, holding them, and the curves of
     the showcase recorded fresh.
2. **Activation.** `convex-core-inputs` imports the seed; `convex-config --demo` compiles
   the books into `DESK-USD`, then stages and activates it. Its fixture asserts every
   bond's opening quote back exactly. A bond on a spread to a Treasury is asserted at
   the Treasury's street yield plus the spread, the yield from the seed's own
   street-convention price, so a wrong convention fails the launch.
3. **The server** in the demo profile, serving the desk app, with the desk's levels on
   source `desk`.
4. **The feed** (`convex-feed-sim`): once a second it moves Treasury yields and credit
   spreads together, posts each priced bond's mid, shifts each desk mark by the same
   moves (a trader's own mark is kept and moves with the market), and publishes each
   SOFR fixing when it falls due.
5. **Once every bond is valued**, each book's limits (market value, DV01, CS01, a key
   rate, issuer and rating concentration, face) and a close of each book, so P&L starts
   at zero.
6. **The simulators**: `convex-rfq-sim` sends clients' requests for quote on the three
   books, and `convex-bms-sim` books each one done.

## What to look at

- **Pricing sheet** (`/desk/sheet`): every bond repricing as the feed moves. Nudge a
  bond's level with `+` and `−`; the feed keeps moving it from there.
- **RFQ blotter** (`/desk/rfqs`): requests arriving with a price off the desk's levels and
  a check against the book's limits. `S` sends, `A` adjusts, `P` passes. A done trade
  changes the book.
- **Books** (`/desk/books`): each book's risk, P&L since the close and limits, live. The
  high yield book has no DV01 or CS01: its callables and perpetuals are valued to their
  workouts, and its floaters by margin, which publish no curve risk.

## Not covered

- Bills: a bill cannot be in the benchmark set, and the spread measures asked of every
  bullet fail for one, so the seed holds none.
- A restart: the database is the work directory's, and a launch begins it again.
- Holidays: the seed settles on the next SIFMA business day and its SOFR fixings are for
  every weekday before it; the feed publishes later ones on the SIFMA calendar.
