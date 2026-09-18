class InvoiceRepository:
    def __init__(self):
        self._rows = {}

    def find(self, invoice_id: str):
        return self._rows.get(invoice_id)

    def save(self, invoice_id: str, row):
        self._rows[invoice_id] = row
        return row
