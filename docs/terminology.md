---
type: Technical Reference
title: Terminology
description: Terms with a specific meaning in this project.
tags: [terminology]
---

# Terminology

Use these terms in code comments and documents. The
[REAS terminology](https://github.com/abenamer/reas/blob/main/standard/terminology.md)
covers the architecture terms.

| Term | Meaning |
| --- | --- |
| Applicant pool | The students who applied to a program, as MySchools counted them last year. |
| Attribute | An eligibility fact a student holds, such as `d15` or `sibling_K442S`. |
| Attrition | A multiplier on published offer rates that models applicants matched elsewhere. |
| Catalog | The MySchools list of schools and programs with last year's demand. |
| Cutoff | The offer rate a program's group reached; a student clears a program when their number is below it. |
| Demand noise | Log-normal year-to-year uncertainty on a cutoff. |
| FRL slice | The part of a group's figures for students eligible for free or reduced price lunch. MySchools reports it separately where a Diversity in Admissions set-aside exists. |
| Initial round | The main match. Offers made here are initial offers. |
| Lottery fraction | The share of the lottery space before a known prefix. |
| Lottery prefix | The known leading hexadecimal characters of a DOE lottery number. |
| Offer share | Offers divided by applicants for one group slice. |
| Priority group | An ordered eligibility tier of a program. A student falls into the first group they qualify for. |
| Program | One admissions program at a school, identified by a code such as `K442S`. |
| Ranking | The student's application list in preference order. |
| Reach | The scan's classification of a program for the student's number. |
| Run | One stored simulation: student, parameters, and report. |
| Screened program | A program that ranks by a talent test; modelled by a percentile draw. |
| Trial | One sampled admissions outcome in the Monte Carlo. |
| Waitlist offer | An offer after the initial round, from a program ranked above the match. |
