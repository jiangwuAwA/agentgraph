from ..deps import get_invoice_service
from ..invoice_service import InvoiceService


def read_invoice(invoice_id: str, svc: InvoiceService = None):
    if svc is None:
        svc = get_invoice_service()
    return {"id": invoice_id, "total": svc.total(invoice_id)}
