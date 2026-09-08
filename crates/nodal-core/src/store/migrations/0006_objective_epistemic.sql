-- How a unit's objective is known: `stated` when a person or an agent said what the
-- unit is for, `observed` when `nodal adopt` recovered it from the records of the
-- session that made the checkout. Null where there is no objective.
--
-- Every unit written before this column existed carries a stated objective, because
-- stating one was the only way to give a unit an objective at all.
ALTER TABLE unit ADD COLUMN objective_epistemic TEXT
    CHECK (objective_epistemic IN ('observed', 'stated'));

UPDATE unit SET objective_epistemic = 'stated' WHERE objective IS NOT NULL;
