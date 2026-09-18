from .invoice_repo import InvoiceRepository


class InvoiceService:
    def __init__(self, repo: InvoiceRepository):
        self.repo = repo

    def total(self, invoice_id: str) -> float:
        row = self.repo.find(invoice_id)
        if row is None:
            return 0.0
        return float(row.get("amount", 0.0))
