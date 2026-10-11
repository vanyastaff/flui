### Removed

- Removed the unused `TickerId` alias and `markers::Ticker` marker from `flui-foundation`, including the scheduler re-export. This is a breaking API removal following the retirement of scheduler tickers in ADR-0175; animation runs are owned by their controllers.
