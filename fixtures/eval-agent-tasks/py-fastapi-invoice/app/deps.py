from .invoice_repo import InvoiceRepository
from .invoice_service import InvoiceService


def get_invoice_repository() -> InvoiceRepository:
    return InvoiceRepository()


def get_invoice_service() -> InvoiceService:
    return InvoiceService(get_invoice_repository())
